//! Port of `Panels/TaskPanelView.swift`: the task panel (§8.5) — the plan,
//! live.
//!
//! "Agent plans are `- [ ]` all the way down."  The panel answers the two
//! questions a reader actually has, in one glance: *how is the plan going*
//! (the section-map bar, one segment per heading) and *let me work it* (the
//! list — open tasks first, tick, quick-add, drag to reorder — every one of
//! them a source edit through the delegate, so the document remains the only
//! source of truth).  *What do I do next* needs no chrome of its own: the list
//! is open-first, so the next task is simply the first row, and Space ticks it
//! before any row is selected.
//!
//! The list is open-first: finished work collapses into a per-section
//! "N completed" pile instead of occupying the top of the panel as a wall of
//! struck-through rows.  Sections group by nearest heading, but a document
//! with one anonymous section skips the header row rather than spend it saying
//! "Document".
//!
//! Toggling and selecting stay separate targets: the checkbox writes to the
//! file immediately, Return (or the hover chevron) jumps the document to the
//! task.  A completion plays its moment — haptic, drawn check, strike sweep —
//! and only then slides the row into the pile, with an Undo pill underneath.
//!
//! Everything in the panel hangs off one left rail (`TaskRowMetrics`): the
//! section bar, the section headers, and the checkbox column all begin at the
//! same x.
//!
//! Objective-C class names equal the Swift ones, the file's private classes
//! included: `TaskPanelView`, `TaskRowSurfaceView` (the base the four row
//! classes subclass; `hoverDidChange` is its overridable Objective-C method),
//! `TaskRowView`, `TaskSectionRowView`, `TaskPileRowView`, `TaskAddRowView`,
//! `TaskImmediateActionButton`, `TaskUndoPillView`.  `InspectorHostView` and
//! `FloatingPanelSurface` reach the panel by selector: `focusForPresentation`,
//! `fittedContentHeight` and `preferredWidth`.
//!
//! Data-structure notes: `rows`, `tasks` and `worklist` are shared snapshots
//! (`Rc`), so a row walk never holds a `RefCell` borrow across AppKit; the
//! height cache (Swift `[Int: CGFloat]` keyed by task index) is a dense
//! vector with NaN for "not measured".

// `!(a > b)` spells Swift's `guard a > b`; the negated comparisons are
// deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAnimatablePropertyContainer, NSBezelStyle, NSButton, NSCellImagePosition, NSColor,
    NSCompositingOperation, NSControl, NSControlSize, NSControlTextEditingDelegate, NSDragOperation, NSDraggingInfo,
    NSEvent, NSEventModifierFlags, NSFocusRingType, NSFont, NSHapticFeedbackManager, NSHapticFeedbackPattern,
    NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer, NSImage, NSImageScaling, NSImageView,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired,
    NSLineBreakMode, NSMenu, NSMenuItem, NSPasteboard, NSPasteboardItem, NSPasteboardTypeString,
    NSPasteboardWriting, NSResponder, NSScrollView, NSTableColumn, NSTableView, NSTableViewAnimationOptions,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewDropOperation, NSTableViewSelectionHighlightStyle,
    NSTextAlignment, NSTextField, NSTextFieldDelegate, NSTextView, NSTrackingArea, NSTrackingAreaOptions,
    NSUserInterfaceItemIdentification, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_foundation::{
    NSAffineTransform, NSArray, NSEdgeInsets, NSIndexSet, NSMutableIndexSet, NSNotFound, NSNotification, NSNumber,
    NSCopying, NSPoint, NSRange, NSRect, NSSize, NSString, NSTimer,
};
use objc2_quartz_core::{
    CABasicAnimation, CAGradientLayer, CAKeyframeAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction,
    CATransaction, CATransform3D, CATransform3DIdentity, CATransition, kCATransitionFade,
};
use upleft_core::NSRange as SourceRange;
use upleft_core::model::{HeadingNode, TaskItem};
use upleft_core::task_worklist::TaskWorklist;
use upleft_render::appkit_compat::{attributed_string, attributes_dictionary, keys, string_bounding_rect};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;
use upleft_swift_text as swift_text;

use super::appkit_support::{
    RECT_ZERO, RectExt, WorkItem, activate, cg, cg_array, configured_symbol, downcast, label, main_async, ns_string,
    null_actions, object, rect, rect_fill, role, set_label, set_mask, set_role, set_value, smax, smin, superview,
    symbol_configuration, system_symbol, weight_bold, weight_regular, weight_semibold, without_actions,
};
use super::panel_chrome::{
    ButtonAction, PanelCheckbox, PanelEmptyStateView, PanelFont, PanelList, PanelMetrics, PanelSurface,
    PanelTableView, element_at, refresh_tracking_area, set_color_values,
};
use super::task_section_bar_view::TaskSectionBarView;

// MARK: - Delegate

/// `TaskPanelDelegate`.
pub trait TaskPanelDelegate {
    /// `markOffset` is the location of the single character between the
    /// brackets, so the host's write is a one-character replacement (§8.5).
    fn task_panel_did_toggle_task_at(&self, panel: &TaskPanelView, mark_offset: isize);
    fn task_panel_did_select_task_at(&self, panel: &TaskPanelView, content_offset: isize);
    /// Quick-add: insert `- [ ] text` at the end of the section's task list.
    fn task_panel_did_request_new_task(&self, panel: &TaskPanelView, text: &str, heading_index: Option<isize>);
    /// Drag reorder within one sibling group.  `target_index == None` moves
    /// the task to the end of its group.
    fn task_panel_did_move_task(&self, panel: &TaskPanelView, task_index: isize, before: Option<isize>);
}

// MARK: - Rows

/// `TaskPanelView.Row`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// Index into `worklist.sections`.
    Section(isize),
    /// Index into `tasks`.
    Task(isize),
    /// The "N completed" disclosure of a section.
    Pile(isize),
    /// Quick-add row of a section; -1 is the whole document (empty plan).
    Add(isize),
}

/// `TaskPanelView.DropTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropTarget {
    Before(isize),
    EndOfGroup,
}

type Handler = RefCell<Option<Rc<dyn Fn()>>>;

pub struct TaskPanelViewIvars {
    delegate: RefCell<Option<Weak<dyn TaskPanelDelegate>>>,
    on_close: Handler,
    /// Fired after the row model changes. The floating owner refits directly
    /// from this signal.
    on_content_size_change: Handler,
    /// The undo pill changes the panel's measured height immediately.
    on_immediate_content_size_change: Handler,
    style_sheet: RefCell<Rc<StyleSheet>>,
    tasks: RefCell<Rc<Vec<TaskItem>>>,
    headings: RefCell<Rc<Vec<HeadingNode>>>,

    section_bar: Retained<TaskSectionBarView>,
    bottom_fade: Retained<CAGradientLayer>,
    scroll_below_section_bar: RefCell<Option<Retained<NSLayoutConstraint>>>,
    scroll_at_top: RefCell<Option<Retained<NSLayoutConstraint>>>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`.
    scroll: OnceCell<Retained<NSScrollView>>,
    empty_state: Retained<PanelEmptyStateView>,
    empty_add_button: Retained<TaskImmediateActionButton>,
    undo_pill: Retained<TaskUndoPillView>,

    worklist: RefCell<Rc<TaskWorklist>>,
    rows: RefCell<Rc<Vec<Row>>>,
    /// Which row carried the selection last, so a selection change can
    /// rebuild two rows instead of the whole list.
    last_selected_row: Cell<Option<isize>>,
    /// Table width the current row heights were measured at.
    measured_width: Cell<CGFloat>,
    /// Swift's `heightCache: [Int: CGFloat]`, by task index; NaN is absent.
    height_cache: RefCell<Vec<CGFloat>>,
    /// Collapse state, keyed by `section_key` so a reload does not reopen
    /// what the reader folded.
    collapsed_sections: RefCell<HashSet<isize>>,
    expanded_piles: RefCell<HashSet<isize>>,
    /// Section whose add-row is editing; -1 is the whole document.
    editing_add_section: Cell<Option<isize>>,
    /// The mark of a completion the panel just asked for.
    pending_completion_mark: Cell<Option<isize>>,
    deferred_rebuild: RefCell<Option<WorkItem>>,
    undo_mark_offset: Cell<Option<isize>>,
    /// Menu actions are target/action pairs; the menu holds no strong
    /// reference, so the panel does for the menu's lifetime.
    menu_actions: RefCell<Vec<Retained<ButtonAction>>>,
    /// What the empty chrome should be, tracked apart from `isHidden` so a
    /// state change during a fade wins over the fade's completion.
    empty_chrome_visible: Cell<bool>,
}

define_class!(
    /// `TaskPanelView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // Swift's `init(frame:)` is unavailable, as here.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskPanelView"]
    #[ivars = TaskPanelViewIvars]
    pub struct TaskPanelView;

    unsafe impl NSObjectProtocol for TaskPanelView {}

    impl TaskPanelView {
        /// `PanelSurface.preferredWidth`, for `FloatingPanelSurface`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        /// For `InspectorHostView`.
        #[unsafe(method(fittedContentHeight))]
        fn __fitted_content_height(&self) -> CGFloat {
            self.fitted_content_height()
        }

        /// For `InspectorHostView`.
        #[unsafe(method(focusForPresentation))]
        fn __focus_for_presentation(&self) {
            self.focus_for_presentation();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.set_style_sheet(Rc::new(StyleSheet::current(self.mtm())));
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.layout_fade();
        }

        /// ⌘N opens the quick-add field.  Caught here rather than in the list
        /// so it works whichever subview is first responder.
        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            if self.handle_quick_add_key(event) {
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn __perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.perform_key_equivalent(event)
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            if self.ivars().editing_add_section.get().is_some() {
                self.cancel_new_task();
            } else {
                let on_close = self.ivars().on_close.borrow().clone();
                if let Some(on_close) = on_close {
                    on_close();
                }
            }
        }

        #[unsafe(method(addTaskFromEmptyState:))]
        fn __add_task_from_empty_state(&self, _sender: Option<&AnyObject>) {
            // The field is the immediate result of the click. Do not leave it
            // hidden beneath the empty-state crossfade.
            self.set_empty_chrome_visible(false, false);
            self.begin_new_task(None);
        }
    }

    unsafe impl NSTableViewDataSource for TaskPanelView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }

        #[unsafe(method_id(tableView:pasteboardWriterForRow:))]
        fn __pasteboard_writer_for_row(
            &self,
            _table_view: &NSTableView,
            row: isize,
        ) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            self.pasteboard_writer_for_row(row)
        }

        #[unsafe(method(tableView:validateDrop:proposedRow:proposedDropOperation:))]
        fn __validate_drop(
            &self,
            table_view: &NSTableView,
            info: &ProtocolObject<dyn NSDraggingInfo>,
            row: isize,
            _drop_operation: NSTableViewDropOperation,
        ) -> NSDragOperation {
            self.validate_drop(table_view, info, row)
        }

        #[unsafe(method(tableView:acceptDrop:row:dropOperation:))]
        fn __accept_drop(
            &self,
            _table_view: &NSTableView,
            info: &ProtocolObject<dyn NSDraggingInfo>,
            row: isize,
            _drop_operation: NSTableViewDropOperation,
        ) -> bool {
            self.accept_drop(info, row)
        }
    }

    unsafe impl NSControlTextEditingDelegate for TaskPanelView {}

    unsafe impl NSTableViewDelegate for TaskPanelView {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }

        #[unsafe(method(tableView:heightOfRow:))]
        fn __height_of_row(&self, table_view: &NSTableView, row: isize) -> CGFloat {
            self.height_of_row(table_view, row)
        }

        /// Only task rows select; headers, piles, and the add row are
        /// controls.
        #[unsafe(method(tableView:shouldSelectRow:))]
        fn __should_select_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            matches!(element_at(&self.rows(), row), Some(Row::Task(_)))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn __selection_did_change(&self, _notification: &NSNotification) {
            self.selection_did_change();
        }
    }
);

impl PanelSurface for TaskPanelView {
    fn preferred_width(&self) -> CGFloat {
        TaskPanelView::preferred_width(self)
    }
}

impl TaskPanelView {
    const BASE_SCROLL_BOTTOM_INSET: CGFloat = 18.0;

    // MARK: - Init

    /// `TaskPanelView()`: hosts build panels before they have a theme in hand
    /// and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<TaskPanelView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<TaskPanelView> {
        // Stored-property initial values, in declaration order, then the
        // `init` body's assignments before `super.init`.
        let bottom_fade = CAGradientLayer::new();
        let table = PanelList::make_table_view("tasks", mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let empty_add_button = TaskImmediateActionButton::with_title("Add Markdown task", mtm);
        let undo_pill = TaskUndoPillView::new(mtm);
        let worklist = TaskWorklist::new(&[], &[]);
        let section_bar = TaskSectionBarView::new(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(TaskPanelViewIvars {
            delegate: RefCell::new(None),
            on_close: RefCell::new(None),
            on_content_size_change: RefCell::new(None),
            on_immediate_content_size_change: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            tasks: RefCell::new(Rc::new(Vec::new())),
            headings: RefCell::new(Rc::new(Vec::new())),
            section_bar,
            bottom_fade,
            scroll_below_section_bar: RefCell::new(None),
            scroll_at_top: RefCell::new(None),
            table,
            scroll: OnceCell::new(),
            empty_state,
            empty_add_button,
            undo_pill,
            worklist: RefCell::new(Rc::new(worklist)),
            rows: RefCell::new(Rc::new(Vec::new())),
            last_selected_row: Cell::new(None),
            measured_width: Cell::new(0.0),
            height_cache: RefCell::new(Vec::new()),
            collapsed_sections: RefCell::new(HashSet::new()),
            expanded_piles: RefCell::new(HashSet::new()),
            editing_add_section: Cell::new(None),
            pending_completion_mark: Cell::new(None),
            deferred_rebuild: RefCell::new(None),
            undo_mark_offset: Cell::new(None),
            menu_actions: RefCell::new(Vec::new()),
            empty_chrome_visible: Cell::new(false),
        });
        let this: Retained<TaskPanelView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };

        this.build_header();
        this.build_table();
        this.build_undo_pill();
        this.install_chrome();
        this.apply_style();
        this.reload();
        this.update_scroll_insets_for_undo_pill();

        set_role(&*this, role::group());
        set_label(&*this, "Tasks");
        this
    }

    /// `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self) -> Retained<NSScrollView> {
        self.ivars()
            .scroll
            .get_or_init(|| PanelList::make_scroll_view(&self.ivars().table, self.mtm()))
            .clone()
    }

    fn build_header(&self) {
        // The toolbar ring owns the whole-plan count. Multi-section documents
        // keep only this map, so the panel never repeats the same meter in
        // text.
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        self.ivars().section_bar.set_on_select_segment(Some(Rc::new(move |index| {
            if let Some(this) = weak.load() {
                this.reveal_section(index);
            }
        })));
    }

    fn build_table(&self) {
        let table = &self.ivars().table;
        // SAFETY: AppKit holds both weakly; the panel owns the table.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        // Task selection only drives the jump affordance. The checkbox owns
        // completion state, so AppKit's cobalt row selection must not flood
        // the task surface when the label is focused.
        table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::None);
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.jump_to_selection();
            }
        })));
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        table.set_on_key_event(Some(Rc::new(move |event: &NSEvent| {
            weak.load().is_some_and(|this| this.handle_quick_add_key(event))
        })));
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        table.set_on_row_mouse_down(Some(Rc::new(move |row: isize| {
            let Some(this) = weak.load() else { return false };
            let rows = this.rows();
            if !(row >= 0 && (row as usize) < rows.len()) {
                return false;
            }
            let Row::Add(_) = rows[row as usize] else { return false };
            this.begin_new_task(None);
            true
        })));
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        table.set_on_key_down(Some(Rc::new(move |key: &str| weak.load().is_some_and(|this| this.handle_list_key(key)))));
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        table.set_on_menu(Some(Rc::new(move |row: isize| weak.load().and_then(|this| this.menu_for_table_row(row)))));
        let types = NSArray::from_slice(&[unsafe { NSPasteboardTypeString }]);
        table.registerForDraggedTypes(&types);
        table.setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        // A little air after the last row, so the plan never ends flush
        // against the glass.
        let scroll = self.scroll();
        scroll.setContentInsets(NSEdgeInsets { top: 0.0, left: 0.0, bottom: Self::BASE_SCROLL_BOTTOM_INSET, right: 0.0 });
        scroll.contentView().setWantsLayer(true);
        let bottom_fade = &self.ivars().bottom_fade;
        unsafe {
            bottom_fade.setColors(Some(&cg_array(&[
                cg(&NSColor::blackColor()),
                cg(&NSColor::blackColor()),
                cg(&NSColor::clearColor()),
            ])));
            bottom_fade.setLocations(Some(&NSArray::from_retained_slice(&[
                NSNumber::new_f64(0.0),
                NSNumber::new_f64(0.86),
                NSNumber::new_f64(1.0),
            ])));
        }
        bottom_fade.setStartPoint(CGPoint::new(0.5, 0.0));
        bottom_fade.setEndPoint(CGPoint::new(0.5, 1.0));
        null_actions(bottom_fade, &["bounds", "position"]);
    }

    fn build_undo_pill(&self) {
        let undo_pill = &self.ivars().undo_pill;
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        undo_pill.set_on_undo(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.undo_completion();
            }
        })));
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        undo_pill.set_on_visibility_change(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.update_scroll_insets_for_undo_pill();
            }
        })));
        undo_pill.setHidden(true);
    }

    fn update_scroll_insets_for_undo_pill(&self) {
        let scroll = self.scroll();
        let mut insets = scroll.contentInsets();
        let bottom = Self::BASE_SCROLL_BOTTOM_INSET
            + if self.ivars().undo_pill.isHidden() { 0.0 } else { TaskUndoPillView::HEIGHT + 12.0 };
        if insets.bottom == bottom {
            return;
        }
        insets.bottom = bottom;
        scroll.setContentInsets(insets);
        let handler = self.ivars().on_immediate_content_size_change.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn install_chrome(&self) {
        let ivars = self.ivars();
        let section_bar = &ivars.section_bar;
        let scroll = self.scroll();
        let undo_pill = &ivars.undo_pill;
        section_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(section_bar);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&scroll);
        undo_pill.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(undo_pill);

        let rail = TaskRowMetrics::CONTENT_INSET;
        *ivars.scroll_below_section_bar.borrow_mut() =
            Some(scroll.topAnchor().constraintEqualToAnchor(&section_bar.bottomAnchor()));
        *ivars.scroll_at_top.borrow_mut() = Some(scroll.topAnchor().constraintEqualToAnchor(&self.topAnchor()));
        activate(&[
            section_bar.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), rail),
            section_bar.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -rail),
            section_bar.topAnchor().constraintEqualToAnchor(&self.topAnchor()),
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
            undo_pill.centerXAnchor().constraintEqualToAnchor(&self.centerXAnchor()),
            undo_pill.bottomAnchor().constraintEqualToAnchor_constant(&self.bottomAnchor(), -12.0),
            undo_pill.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&self.leadingAnchor(), 12.0),
            undo_pill.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&self.trailingAnchor(), -12.0),
            undo_pill.heightAnchor().constraintEqualToConstant(TaskUndoPillView::HEIGHT),
        ]);
        self.update_section_chrome_visibility();

        // Installed last so "nothing here yet" floats over the (empty) list
        // in the same place every panel puts it.  The button belongs to the
        // same visual group. Lift the state so the combined state + action,
        // not the text block alone, is centred.
        let empty_state = &ivars.empty_state;
        empty_state.install(self, &scroll, 0.88);

        let button = &ivars.empty_add_button;
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Small);
        button.setFont(Some(&PanelFont::system(12.0, weight_semibold())));
        button.setImage(configured_symbol("plus", Some("Add task"), &symbol_configuration(11.0, weight_semibold())).as_deref());
        button.setImagePosition(NSCellImagePosition::ImageLeading);
        unsafe {
            button.setTarget(Some(object(self)));
            button.setAction(Some(sel!(addTaskFromEmptyState:)));
        }
        set_role(&**button, role::button());
        set_label(&**button, "Add Markdown task");
        button.setToolTip(Some(&ns_string("Insert a - [ ] checkbox into this document")));
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(button);
        activate(&[
            button.topAnchor().constraintEqualToAnchor_constant(&empty_state.bottomAnchor(), 10.0),
            button.centerXAnchor().constraintEqualToAnchor(&self.centerXAnchor()),
            button.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            button.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            button.heightAnchor().constraintEqualToConstant(28.0),
        ]);
        button.setHidden(true);
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn TaskPanelDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn TaskPanelDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn set_on_close(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_close.borrow_mut() = handler;
    }

    pub fn set_on_content_size_change(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_content_size_change.borrow_mut() = handler;
    }

    pub fn set_on_immediate_content_size_change(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_immediate_content_size_change.borrow_mut() = handler;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// `styleSheet` with its `didSet`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().section_bar.set_style_sheet(style_sheet.clone());
        self.ivars().undo_pill.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn tasks(&self) -> Rc<Vec<TaskItem>> {
        self.ivars().tasks.borrow().clone()
    }

    /// `tasks` with its `didSet` (`reload()`).
    pub fn set_tasks(&self, tasks: impl Into<Rc<Vec<TaskItem>>>) {
        *self.ivars().tasks.borrow_mut() = tasks.into();
        self.reload();
    }

    pub fn headings(&self) -> Rc<Vec<HeadingNode>> {
        self.ivars().headings.borrow().clone()
    }

    /// `headings` with its `didSet` (`reload()`).
    pub fn set_headings(&self, headings: impl Into<Rc<Vec<HeadingNode>>>) {
        *self.ivars().headings.borrow_mut() = headings.into();
        self.reload();
    }

    fn worklist(&self) -> Rc<TaskWorklist> {
        self.ivars().worklist.borrow().clone()
    }

    fn rows(&self) -> Rc<Vec<Row>> {
        self.ivars().rows.borrow().clone()
    }

    /// `progress`: `(done, total)`.
    pub fn progress(&self) -> (isize, isize) {
        let worklist = self.worklist();
        (worklist.done_count, worklist.total_count)
    }

    /// A worklist is a narrow tool, not a working surface: the slim panel
    /// width, one tier below the detail panels.
    pub fn preferred_width(&self) -> CGFloat {
        300.0
    }

    /// The height the current content wants at its current width: the row
    /// model's actual heights plus the list insets and panel chrome.
    pub fn fitted_content_height(&self) -> CGFloat {
        self.layoutSubtreeIfNeeded();
        // Presentation can measure in the same run-loop turn that a parse
        // replaces the worklist. A deferred visual diff must not make layout
        // believe a forty-row plan is still the previous one-row list.
        let measured_rows = self.build_rows();
        if *self.rows() != measured_rows {
            *self.ivars().rows.borrow_mut() = Rc::new(measured_rows);
            self.ivars().table.reloadData();
        }
        let width = self.measurement_width(self.ivars().table.bounds().width());
        let rows = self.rows();
        let list_height = if rows.is_empty() {
            Self::empty_state_content_height(&self.ivars().empty_state)
        } else {
            self.rows_height(&rows, width)
        };
        let insets = self.scroll().contentInsets();
        // Measurement can run before the scroll view joins a window. Keep the
        // authored footer clearance as the floor, plus a small optical gap so
        // the final row never touches the glass rim at fractional scales.
        let footer_clearance = smax(Self::BASE_SCROLL_BOTTOM_INSET, insets.bottom) + 4.0;
        let section_map_height = if self.shows_section_headers() {
            self.ivars().section_bar.intrinsicContentSize().height
        } else {
            0.0
        };
        section_map_height + insets.top + list_height + footer_clearance
    }

    /// The vertical space the empty state occupies: its state block, the gap
    /// to the add button, the button, and the margin that keeps the button
    /// clear of the card's rounded lower edge.
    pub fn empty_state_content_height(empty_state: &PanelEmptyStateView) -> CGFloat {
        empty_state.fittingSize().height
            + 10.0 // gap: state → button
            + 28.0 // add-task button
            + 20.0 // optical clearance from the rounded lower edge
    }

    /// `rows.indices.reduce(0) { $0 + rowHeight($1, width: width) }`.
    fn rows_height(&self, rows: &[Row], width: CGFloat) -> CGFloat {
        let mut total: CGFloat = 0.0;
        for index in 0..rows.len() {
            total += self.row_height(rows, index as isize, width);
        }
        total
    }

    // MARK: - Testing

    pub fn visible_task_count_for_testing(&self) -> isize {
        self.rows().iter().filter(|row| matches!(row, Row::Task(_))).count() as isize
    }

    pub fn pile_row_count_for_testing(&self) -> isize {
        self.rows().iter().filter(|row| matches!(row, Row::Pile(_))).count() as isize
    }

    pub fn status_line_for_testing(&self) -> String {
        self.worklist().status_line.clone()
    }

    pub fn caption_for_testing(&self) -> String {
        self.worklist().count_line.clone()
    }

    pub fn row_count_for_testing(&self) -> isize {
        self.rows().len() as isize
    }

    pub fn content_document_height_for_testing(&self) -> CGFloat {
        self.scroll().documentView().map_or(0.0, |view| view.bounds().height())
    }

    pub fn content_viewport_height_for_testing(&self) -> CGFloat {
        self.scroll().contentView().bounds().height()
    }

    pub fn measured_list_height_for_testing(&self) -> CGFloat {
        let width = self.measurement_width(self.ivars().table.bounds().width());
        let rows = self.rows();
        if rows.is_empty() {
            Self::empty_state_content_height(&self.ivars().empty_state)
        } else {
            self.rows_height(&rows, width)
        }
    }

    fn measurement_width(&self, table_width: CGFloat) -> CGFloat {
        if table_width > 1.0 {
            return table_width;
        }
        let width = self.bounds().width();
        if width > 1.0 {
            return width;
        }
        self.preferred_width()
    }

    pub fn empty_add_button_for_testing(&self) -> Retained<NSButton> {
        Retained::into_super(self.ivars().empty_add_button.clone())
    }

    pub fn quick_add_editing_for_testing(&self) -> bool {
        self.ivars().editing_add_section.get().is_some()
    }

    pub fn perform_add_row_accessibility_press_for_testing(&self) -> bool {
        let Some(row) = self.rows().iter().position(|row| matches!(row, Row::Add(_))) else { return false };
        self.ivars()
            .table
            .viewAtColumn_row_makeIfNecessary(0, row as isize, true)
            .and_then(|view| downcast::<TaskAddRowView>(&view))
            .is_some_and(|view| unsafe { msg_send![&*view, accessibilityPerformPress] })
    }

    pub fn commit_new_task_for_testing(&self, text: &str) {
        self.commit_new_task(text);
    }

    pub fn undo_bottom_inset_for_testing(&self) -> CGFloat {
        self.scroll().contentInsets().bottom
    }

    pub fn undo_required_bottom_inset_for_testing(&self) -> CGFloat {
        Self::BASE_SCROLL_BOTTOM_INSET + TaskUndoPillView::HEIGHT + 12.0
    }

    pub fn undo_pill_frame_for_testing(&self) -> NSRect {
        self.ivars().undo_pill.frame()
    }

    pub fn last_row_frame_for_testing(&self) -> Option<NSRect> {
        let table = &self.ivars().table;
        if !(table.numberOfRows() > 0) {
            return None;
        }
        Some(table.convertRect_toView(table.rectOfRow(table.numberOfRows() - 1), Some(self)))
    }

    /// `presentUndoForTesting(title:)`; Swift's default title is "Task".
    pub fn present_undo_for_testing(&self, title: &str) {
        self.ivars().undo_pill.present(title);
        self.update_scroll_insets_for_undo_pill();
    }

    pub fn dismiss_undo_for_testing(&self) {
        self.ivars().undo_pill.dismiss(false);
    }

    pub fn set_completed_pile_expanded_for_testing(&self, expanded: bool, section: isize) {
        let key = self.section_key(section);
        if expanded {
            self.ivars().expanded_piles.borrow_mut().insert(key);
        } else {
            self.ivars().expanded_piles.borrow_mut().remove(&key);
        }
        self.reload();
    }

    // MARK: - Style

    fn apply_style(&self) {
        let ivars = self.ivars();
        if !ivars.empty_state.isHidden() {
            self.configure_empty_state();
        }
        // Cell views own their theme colours. A live theme switch must
        // reconfigure the visible cells.
        let table = &ivars.table;
        if !(table.numberOfRows() > 0) {
            return;
        }
        let visible = table.rowsInRect(table.visibleRect());
        if !(visible.location != NSNotFound as usize && visible.length > 0) {
            return;
        }
        let end = ((visible.location + visible.length) as isize).min(table.numberOfRows());
        assert!(visible.location as isize <= end, "Range requires lowerBound <= upperBound");
        let rows = NSIndexSet::indexSetWithIndexesInRange(NSRange::new(visible.location, end as usize - visible.location));
        table.reloadDataForRowIndexes_columnIndexes(&rows, &NSIndexSet::indexSetWithIndex(0));
    }

    // MARK: - Content

    pub fn reload(&self) {
        let worklist = TaskWorklist::new(&self.tasks(), &self.headings());
        *self.ivars().worklist.borrow_mut() = Rc::new(worklist);
        self.ivars().height_cache.borrow_mut().clear();
        self.update_summary();
        self.update_section_chrome_visibility();
        self.schedule_row_rebuild();
        self.update_accessibility();
    }

    fn update_section_chrome_visibility(&self) {
        let visible = self.shows_section_headers();
        self.ivars().section_bar.setHidden(!visible);
        let below = self.ivars().scroll_below_section_bar.borrow().clone();
        let at_top = self.ivars().scroll_at_top.borrow().clone();
        below.expect("scrollBelowSectionBar").setActive(visible);
        at_top.expect("scrollAtTop").setActive(!visible);
    }

    /// `layout()` after `super.layout()`.
    fn layout_fade(&self) {
        let clip = self.scroll().contentView();
        // NSTableView expands its document view to the viewport when the
        // list is short, and NSScrollView folds content insets into that
        // geometry.  Compare the authored row model with the usable viewport
        // instead; the fade appears only when there is real offscreen work to
        // reveal.
        let width = self.measurement_width(self.ivars().table.bounds().width());
        let rows = self.rows();
        let rows_height = self.rows_height(&rows, width);
        // `NSClipView.bounds` is already the usable content viewport.
        let overflows = rows_height > clip.bounds().height() + 0.5;
        let bottom_fade = &self.ivars().bottom_fade;
        if overflows {
            bottom_fade.setFrame(clip.bounds());
            if let Some(layer) = clip.layer() {
                set_mask(&layer, Some(bottom_fade));
            }
        } else if let Some(layer) = clip.layer()
            && layer.mask().is_some_and(|mask| std::ptr::eq(&*mask, &***bottom_fade))
        {
            set_mask(&layer, None);
        }
    }

    fn update_summary(&self) {
        self.ivars().section_bar.set_segments(self.worklist().segments.clone());
        // The tally rides at the bar's trailing end; the meter holds still
        // under the pointer, and a click still scrolls to the section.
        self.update_empty_state();
    }

    fn update_empty_state(&self) {
        let empty = self.worklist().total_count == 0 && self.ivars().editing_add_section.get().is_none();
        if empty {
            self.configure_empty_state();
        }
        self.set_empty_chrome_visible(empty, self.window().is_some() && !self.style_sheet().reduce_motion);
    }

    /// The empty state crossfades rather than popping.
    fn set_empty_chrome_visible(&self, visible: bool, animated: bool) {
        let ivars = self.ivars();
        if visible == ivars.empty_chrome_visible.get() {
            return;
        }
        ivars.empty_chrome_visible.set(visible);
        let chrome: [Retained<NSView>; 2] = [
            Retained::into_super(ivars.empty_state.clone()),
            Retained::into_super(Retained::into_super(Retained::into_super(ivars.empty_add_button.clone()))),
        ];
        if visible {
            for view in &chrome {
                view.setAlphaValue(if animated { 0.0 } else { 1.0 });
                view.setHidden(false);
            }
            if !animated {
                return;
            }
            motion::run(
                false,
                motion::STANDARD,
                Curve::EaseOut,
                move |_| {
                    for view in &chrome {
                        view.animator().setAlphaValue(1.0);
                    }
                },
                None,
            );
        } else {
            if !animated {
                for view in &chrome {
                    view.setHidden(true);
                    view.setAlphaValue(1.0);
                }
                return;
            }
            let fading = chrome.clone();
            let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
            motion::run(
                false,
                motion::STANDARD,
                Curve::EaseOut,
                move |_| {
                    for view in &fading {
                        view.animator().setAlphaValue(0.0);
                    }
                },
                Some(Box::new(move || {
                    let Some(this) = weak.load() else { return };
                    if this.ivars().empty_chrome_visible.get() {
                        return;
                    }
                    for view in &chrome {
                        view.setHidden(true);
                        view.setAlphaValue(1.0);
                    }
                })),
            );
        }
    }

    fn configure_empty_state(&self) {
        self.ivars().empty_state.configure(
            "checklist",
            "No tasks yet",
            "Tasks are Markdown checkboxes. Add one here or type “- [ ]”.",
            &self.style_sheet(),
        );
    }

    fn update_accessibility(&self) {
        if self.ivars().editing_add_section.get().is_some() {
            set_value(self, "New task title");
        } else {
            let worklist = self.worklist();
            set_value(self, if worklist.status_line.is_empty() { "No tasks" } else { &worklist.status_line });
        }
    }

    // MARK: - Rows

    fn section_key(&self, section: isize) -> isize {
        let worklist = self.worklist();
        if !(section >= 0 && (section as usize) < worklist.sections.len()) {
            return -1;
        }
        worklist.sections[section as usize].heading_index.unwrap_or(-1)
    }

    /// A section header earns its row only when there is something to tell
    /// apart.
    fn shows_section_headers(&self) -> bool {
        self.ivars().worklist.borrow().sections.len() > 1
    }

    fn build_rows(&self) -> Vec<Row> {
        let worklist = self.worklist();
        let shows_section_headers = worklist.sections.len() > 1;
        let mut result: Vec<Row> = Vec::new();
        {
            let collapsed_sections = self.ivars().collapsed_sections.borrow();
            let expanded_piles = self.ivars().expanded_piles.borrow();
            for (index, section) in worklist.sections.iter().enumerate() {
                let index = index as isize;
                if shows_section_headers {
                    result.push(Row::Section(index));
                }
                let collapsed = shows_section_headers && collapsed_sections.contains(&self.section_key(index));
                if !collapsed {
                    for entry in &section.open_entries {
                        result.push(Row::Task(entry.task_index));
                    }
                    if !section.done_entries.is_empty() {
                        // The pile keeps finished work from becoming a wall of
                        // check marks above the work that is left.  A section
                        // with *nothing* left has no such wall to hold back.
                        let has_open_work = !section.open_entries.is_empty();
                        let expanded = expanded_piles.contains(&self.section_key(index));
                        if has_open_work {
                            result.push(Row::Pile(index));
                        }
                        if !has_open_work || expanded {
                            for entry in &section.done_entries {
                                result.push(Row::Task(entry.task_index));
                            }
                        }
                    }
                }
            }
        }
        // One add row for the whole panel, at its foot.
        let editing = self.ivars().editing_add_section.get();
        if !worklist.sections.is_empty() || editing.is_some() {
            result.push(Row::Add(editing.unwrap_or_else(|| self.preferred_add_section())));
        }
        result
    }

    /// The completion moment holds the list still for a beat: the row the
    /// reader just ticked keeps its place while the check draws and the
    /// strike sweeps, then the rebuild slides it into the pile.
    fn schedule_row_rebuild(&self) {
        let ivars = self.ivars();
        let previous = ivars.deferred_rebuild.borrow_mut().take();
        if let Some(previous) = previous {
            previous.cancel();
        }
        let has_window = self.window().is_some();
        let Some(mark) = ivars.pending_completion_mark.get().filter(|_| has_window && !self.style_sheet().reduce_motion)
        else {
            ivars.pending_completion_mark.set(None);
            self.rebuild_rows(has_window);
            return;
        };
        // Only hold when the completion actually landed in the new parse.
        let landed = self.tasks().iter().find(|task| task.mark_range.location == mark).map(|task| task.is_checked);
        if landed != Some(true) {
            ivars.pending_completion_mark.set(None);
            self.rebuild_rows(has_window);
            return;
        }
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            this.ivars().pending_completion_mark.set(None);
            *this.ivars().deferred_rebuild.borrow_mut() = None;
            this.rebuild_rows(true);
        });
        *ivars.deferred_rebuild.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(0.10);
    }

    fn rebuild_rows(&self, animated: bool) {
        let selected_task = self.selected_task_index();
        let new_rows = Rc::new(self.build_rows());
        let old = self.rows();
        *self.ivars().rows.borrow_mut() = new_rows.clone();

        let table = &self.ivars().table;
        let can_animate = animated
            && self.window().is_some()
            && !self.style_sheet().reduce_motion
            && !old.is_empty()
            && !new_rows.is_empty();
        if !(can_animate && new_rows != old) {
            table.reloadData();
            self.restore_selection(selected_task);
            self.notify_content_size_change();
            return;
        }

        let (removed, inserted) = collection_difference(&old, &new_rows);
        // Removals address the old row model, insertions the new one —
        // exactly the coordinate spaces `beginUpdates` expects.
        let removals = NSMutableIndexSet::new();
        for offset in &removed {
            removals.addIndex(*offset);
        }
        let insertions = NSMutableIndexSet::new();
        for offset in &inserted {
            insertions.addIndex(*offset);
        }
        table.beginUpdates();
        table.removeRowsAtIndexes_withAnimation(
            &removals,
            NSTableViewAnimationOptions::EffectFade | NSTableViewAnimationOptions::SlideUp,
        );
        table.insertRowsAtIndexes_withAnimation(
            &insertions,
            NSTableViewAnimationOptions::EffectFade | NSTableViewAnimationOptions::SlideDown,
        );
        table.endUpdates();
        // Survivors kept their row identity; their contents may still have
        // changed (a pile's count, a task the document edited in place).
        let survivors = NSMutableIndexSet::new();
        for index in 0..new_rows.len() {
            if !insertions.containsIndex(index) {
                survivors.addIndex(index);
            }
        }
        if survivors.count() != 0 {
            table.reloadDataForRowIndexes_columnIndexes(&survivors, &NSIndexSet::indexSetWithIndex(0));
        }
        self.restore_selection(selected_task);
        self.notify_content_size_change();
    }

    fn notify_content_size_change(&self) {
        let handler = self.ivars().on_content_size_change.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn restore_selection(&self, task_index: Option<isize>) {
        let row = task_index.and_then(|task_index| self.rows().iter().position(|row| *row == Row::Task(task_index)));
        let Some(row) = row else {
            self.ivars().last_selected_row.set(None);
            return;
        };
        let table = &self.ivars().table;
        if table.selectedRow() != row as isize {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
        }
        self.ivars().last_selected_row.set(Some(row as isize));
    }

    fn selected_task_index(&self) -> Option<isize> {
        let selected = self.ivars().table.selectedRow();
        let rows = self.rows();
        if !(selected >= 0 && (selected as usize) < rows.len()) {
            return None;
        }
        match rows[selected as usize] {
            Row::Task(index) => Some(index),
            _ => None,
        }
    }

    // MARK: - Actions

    fn toggle_task(&self, task: &TaskItem) {
        let completing = !task.is_checked;
        if completing {
            NSHapticFeedbackManager::defaultPerformer()
                .performFeedbackPattern_performanceTime(NSHapticFeedbackPattern::Generic, NSHapticFeedbackPerformanceTime::Now);
            self.ivars().pending_completion_mark.set(Some(task.mark_range.location));
            self.ivars().undo_mark_offset.set(Some(task.mark_range.location));
            self.ivars().undo_pill.present(&task.text);
            self.update_scroll_insets_for_undo_pill();
        } else {
            self.ivars().pending_completion_mark.set(None);
        }
        if let Some(delegate) = self.delegate() {
            delegate.task_panel_did_toggle_task_at(self, task.mark_range.location);
        }
    }

    fn undo_completion(&self) {
        let Some(mark) = self.ivars().undo_mark_offset.get() else { return };
        let tasks = self.tasks();
        let Some(task) = tasks.iter().find(|task| task.mark_range.location == mark) else { return };
        if !task.is_checked {
            return;
        }
        // An un-tick is a plain toggle, not a completion: no pill, no hold.
        self.ivars().pending_completion_mark.set(None);
        if let Some(delegate) = self.delegate() {
            delegate.task_panel_did_toggle_task_at(self, mark);
        }
    }

    fn toggle_up_next(&self) -> bool {
        let worklist = self.worklist();
        let tasks = self.tasks();
        let Some(up) = worklist.up_next.as_ref().filter(|up| up.entry.task_index < tasks.len() as isize) else {
            return false;
        };
        let task = tasks[up.entry.task_index as usize].clone();
        self.toggle_task(&task);
        true
    }

    fn jump_to_up_next(&self) -> bool {
        let worklist = self.worklist();
        let Some(up) = worklist.up_next.as_ref() else { return false };
        if let Some(delegate) = self.delegate() {
            delegate.task_panel_did_select_task_at(self, up.entry.content_offset);
        }
        true
    }

    fn jump_to_selection(&self) {
        let tasks = self.tasks();
        let Some(index) = self.selected_task_index().filter(|&index| index < tasks.len() as isize) else { return };
        if let Some(delegate) = self.delegate() {
            delegate.task_panel_did_select_task_at(self, tasks[index as usize].content_range.location);
        }
    }

    /// A click on a section-map segment: unfold the section if the reader had
    /// folded it, then bring its header into view.
    fn reveal_section(&self, section: isize) {
        if !(section < self.worklist().sections.len() as isize) {
            return;
        }
        let key = self.section_key(section);
        let removed = self.ivars().collapsed_sections.borrow_mut().remove(&key);
        if removed {
            self.rebuild_rows(true);
        }
        if let Some(row) = self.rows().iter().position(|row| *row == Row::Section(section)) {
            self.ivars().table.scrollRowToVisible(row as isize);
        }
    }

    // MARK: - Quick add

    /// ⌘N, or a click on the "Add task" row.  The field opens in the section
    /// the reader is looking at — the selected task's, then Up Next's, then
    /// the plan's last; an empty plan gets one document-scope row.
    fn begin_new_task(&self, requested: Option<isize>) {
        if self.ivars().editing_add_section.get().is_some() {
            return;
        }
        let section = requested.unwrap_or_else(|| self.preferred_add_section());
        if section >= 0 {
            let key = self.section_key(section);
            self.ivars().collapsed_sections.borrow_mut().remove(&key);
        }
        self.ivars().editing_add_section.set(Some(section));
        self.rebuild_rows(false);
        self.update_empty_state();
        self.update_accessibility();
        self.focus_add_field(section);
    }

    pub fn begin_new_task_for_command(&self) {
        self.begin_new_task(None);
    }

    fn preferred_add_section(&self) -> isize {
        let worklist = self.worklist();
        if let Some(index) = self.selected_task_index() {
            let tasks = self.tasks();
            let heading_index = tasks[index as usize].heading_index;
            if let Some(section) = worklist.sections.iter().position(|section| section.heading_index == heading_index) {
                return section as isize;
            }
        }
        if let Some(up) = &worklist.up_next {
            return up.section_index;
        }
        if worklist.sections.is_empty() { -1 } else { worklist.sections.len() as isize - 1 }
    }

    fn focus_add_field(&self, section: isize) {
        let Some(row) = self.rows().iter().position(|row| *row == Row::Add(section)) else { return };
        let row = row as isize;
        self.ivars().table.scrollRowToVisible(row);
        // The field exists once the row view is on screen; first-responder
        // has to wait a turn for the table to lay the row out.
        let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
        main_async(move || {
            let Some(this) = weak.load() else { return };
            let view = this
                .ivars()
                .table
                .viewAtColumn_row_makeIfNecessary(0, row, true)
                .and_then(|view| downcast::<TaskAddRowView>(&view));
            if let Some(view) = view
                && let Some(window) = this.window()
            {
                window.makeFirstResponder(Some(&view.ivars().text_field));
            }
        });
    }

    fn commit_new_task(&self, text: &str) {
        let section = self.ivars().editing_add_section.get();
        self.ivars().editing_add_section.set(None);
        // The reparse delivers the new row; this reload just closes the
        // field, which must also happen when the insert itself is refused
        // (Swift's `defer { reload() }`).
        if let Some(section) = section
            && !swift_text::trim_whitespaces_and_newlines(text).is_empty()
        {
            let worklist = self.worklist();
            let heading = if section >= 0 && (section as usize) < worklist.sections.len() {
                worklist.sections[section as usize].heading_index
            } else {
                None
            };
            if let Some(delegate) = self.delegate() {
                delegate.task_panel_did_request_new_task(self, text, heading);
            }
        }
        self.reload();
    }

    fn cancel_new_task(&self) {
        self.ivars().editing_add_section.set(None);
        self.reload();
    }

    // MARK: - Context menu

    fn menu_item(&self, title: &str, handler: impl Fn() + 'static) -> Retained<NSMenuItem> {
        let mtm = self.mtm();
        let action = ButtonAction::new(handler, mtm);
        self.ivars().menu_actions.borrow_mut().push(action.clone());
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_string(title),
                Some(ButtonAction::selector()),
                &NSString::from_str(""),
            )
        };
        unsafe { item.setTarget(Some(object(&*action))) };
        item
    }

    fn menu_for_table_row(&self, row: isize) -> Option<Retained<NSMenu>> {
        let rows = self.rows();
        let row_kind = *element_at(&rows, row)?;
        self.ivars().menu_actions.borrow_mut().clear();
        let mtm = self.mtm();
        let tasks = self.tasks();
        match row_kind {
            Row::Task(index) if index < tasks.len() as isize => {
                let task = tasks[index as usize].clone();
                self.ivars().table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
                let menu = NSMenu::new(mtm);
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                let location = task.content_range.location;
                menu.addItem(&self.menu_item("Jump to Source", move || {
                    let Some(this) = weak.load() else { return };
                    if let Some(delegate) = this.delegate() {
                        delegate.task_panel_did_select_task_at(&this, location);
                    }
                }));
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                let toggled = task.clone();
                menu.addItem(&self.menu_item(if task.is_checked { "Mark Incomplete" } else { "Mark Complete" }, move || {
                    if let Some(this) = weak.load() {
                        this.toggle_task(&toggled);
                    }
                }));
                menu.addItem(&NSMenuItem::separatorItem(mtm));
                let text = task.text.clone();
                menu.addItem(&self.menu_item("Copy Task Text", move || {
                    let pasteboard = NSPasteboard::generalPasteboard();
                    pasteboard.clearContents();
                    pasteboard.setString_forType(&ns_string(&text), unsafe { NSPasteboardTypeString });
                }));
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                menu.addItem(&self.menu_item("Copy Status Report", move || {
                    if let Some(this) = weak.load() {
                        this.copy_status_report();
                    }
                }));
                Some(menu)
            }
            Row::Section(_) => {
                let menu = NSMenu::new(mtm);
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                menu.addItem(&self.menu_item("Copy Status Report", move || {
                    if let Some(this) = weak.load() {
                        this.copy_status_report();
                    }
                }));
                Some(menu)
            }
            Row::Pile(section) => {
                // The pile is a disclosure control; its menu is the same fold,
                // plus the report every other section surface already offers.
                let key = self.section_key(section);
                let expanded = self.ivars().expanded_piles.borrow().contains(&key);
                let menu = NSMenu::new(mtm);
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                menu.addItem(&self.menu_item(if expanded { "Collapse Completed" } else { "Show Completed" }, move || {
                    let Some(this) = weak.load() else { return };
                    if expanded {
                        this.ivars().expanded_piles.borrow_mut().remove(&key);
                    } else {
                        this.ivars().expanded_piles.borrow_mut().insert(key);
                    }
                    this.rebuild_rows(true);
                }));
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                menu.addItem(&self.menu_item("Copy Status Report", move || {
                    if let Some(this) = weak.load() {
                        this.copy_status_report();
                    }
                }));
                Some(menu)
            }
            _ => None,
        }
    }

    /// The status report is Markdown, like everything else here.
    fn copy_status_report(&self) {
        let worklist = self.worklist();
        if worklist.status_report.is_empty() {
            return;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        pasteboard.setString_forType(&ns_string(&worklist.status_report), unsafe { NSPasteboardTypeString });
    }

    // MARK: - Keyboard

    fn handle_quick_add_key(&self, event: &NSEvent) -> bool {
        let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        // Key codes identify physical keys, not the character produced by the
        // active layout. Use the translated command character.
        let is_command_n = event
            .charactersIgnoringModifiers()
            .is_some_and(|characters| swift_text::str_eq(&swift_text::lowercased(&characters.to_string()), "n"));
        if !(flags.contains(NSEventModifierFlags::Command)
            && !flags.intersects(NSEventModifierFlags::Shift | NSEventModifierFlags::Option | NSEventModifierFlags::Control)
            && is_command_n
            && self.ivars().editing_add_section.get().is_none())
        {
            return false;
        }
        self.begin_new_task(None);
        true
    }

    fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
        if self.handle_quick_add_key(event) {
            return true;
        }
        unsafe { msg_send![super(self), performKeyEquivalent: event] }
    }

    /// The list's keys.  Space ticks the selected task — or Up Next when
    /// nothing is selected; Return jumps; ←/→ fold and unfold sections and
    /// piles.
    fn handle_list_key(&self, key: &str) -> bool {
        match key {
            "space" => {
                let tasks = self.tasks();
                if let Some(index) = self.selected_task_index().filter(|&index| index < tasks.len() as isize) {
                    let task = tasks[index as usize].clone();
                    self.toggle_task(&task);
                    return true;
                }
                self.toggle_up_next()
            }
            "return" => {
                if self.ivars().table.selectedRow() < 0 {
                    return self.jump_to_up_next();
                }
                false // a selected row's Return is the table's onActivate
            }
            "left" | "right" => self.adjust_disclosure(key == "right"),
            _ => false,
        }
    }

    fn adjust_disclosure(&self, expanding: bool) -> bool {
        let row = self.ivars().table.selectedRow();
        let rows = self.rows();
        let Some(&row_kind) = element_at(&rows, row) else { return false };
        match row_kind {
            Row::Section(section) => {
                let key = self.section_key(section);
                if expanding {
                    self.ivars().collapsed_sections.borrow_mut().remove(&key);
                } else {
                    self.ivars().collapsed_sections.borrow_mut().insert(key);
                }
                self.rebuild_rows(true);
                true
            }
            Row::Pile(section) => {
                let key = self.section_key(section);
                if expanding {
                    self.ivars().expanded_piles.borrow_mut().insert(key);
                } else {
                    self.ivars().expanded_piles.borrow_mut().remove(&key);
                }
                self.rebuild_rows(true);
                true
            }
            Row::Task(index) if !expanding => {
                // ← on a task walks the selection up to its section header.
                let heading_index = self.tasks()[index as usize].heading_index;
                let Some(section) =
                    self.worklist().sections.iter().position(|section| section.heading_index == heading_index)
                else {
                    return false;
                };
                let Some(header) = rows.iter().position(|row| *row == Row::Section(section as isize)) else {
                    return false;
                };
                self.ivars().table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(header), false);
                true
            }
            _ => false,
        }
    }

    /// The floating owner gives the list the first key loop position.
    pub fn focus_for_presentation(&self) {
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&self.ivars().table));
        }
    }

    // MARK: - Drag reorder

    /// The group a drag may move inside: same heading, same indent, same
    /// parent — `Restructure.moveTask`'s sibling rule.  A negative index
    /// traps, as Swift's subscript does.
    fn sibling_key(&self, index: isize) -> String {
        let tasks = self.tasks();
        if !(index < tasks.len() as isize) {
            return String::new();
        }
        let task = &tasks[index as usize];
        format!(
            "{}#{}#{}",
            task.heading_index.unwrap_or(-1),
            task.indent_level,
            self.parent_task_index(index).unwrap_or(-1)
        )
    }

    fn parent_task_index(&self, index: isize) -> Option<isize> {
        let tasks = self.tasks();
        if !(index < tasks.len() as isize) {
            return None;
        }
        let task = &tasks[index as usize];
        let mut candidate = index - 1;
        while candidate >= 0 {
            let other = &tasks[candidate as usize];
            if other.heading_index == task.heading_index && other.indent_level < task.indent_level {
                return Some(candidate);
            }
            candidate -= 1;
        }
        None
    }

    /// The task a gap-drop would precede, or `EndOfGroup` at the group's
    /// tail; `None` when the gap is outside the dragged task's sibling group.
    fn drop_before_target(&self, gap: isize, source: isize) -> Option<DropTarget> {
        let key = self.sibling_key(source);
        let rows = self.rows();
        let mut above: Option<isize> = None;
        let mut row = gap - 1;
        while row >= 0 {
            match rows[row as usize] {
                Row::Task(index) => {
                    above = Some(index);
                    break;
                }
                Row::Section(_) => break,
                _ => {}
            }
            row -= 1;
        }
        let mut below: Option<isize> = None;
        row = gap;
        while row < rows.len() as isize {
            match rows[row as usize] {
                Row::Task(index) => {
                    below = Some(index);
                    break;
                }
                Row::Section(_) => break,
                _ => {}
            }
            row += 1;
        }
        // Dropping on the task's own slot is a no-op the edit would refuse.
        if above == Some(source) || below == Some(source) {
            return None;
        }
        if let Some(below) = below
            && self.sibling_key(below) == key
        {
            return Some(DropTarget::Before(below));
        }
        if let Some(above) = above
            && self.sibling_key(above) == key
        {
            return Some(DropTarget::EndOfGroup);
        }
        None
    }

    fn task_height(&self, task_index: isize, width: CGFloat) -> CGFloat {
        let ivars = self.ivars();
        if ivars.measured_width.get() != width {
            ivars.measured_width.set(width);
            ivars.height_cache.borrow_mut().clear();
        }
        let index = task_index as usize;
        if let Some(&cached) = ivars.height_cache.borrow().get(index)
            && !cached.is_nan()
        {
            return cached;
        }
        let height = {
            let tasks = self.tasks();
            TaskRowView::fitting_height(&tasks[index], width, &self.style_sheet())
        };
        let mut cache = ivars.height_cache.borrow_mut();
        if cache.len() <= index {
            cache.resize(self.ivars().tasks.borrow().len().max(index + 1), CGFloat::NAN);
        }
        cache[index] = height;
        height
    }

    // MARK: - Table

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let rows = self.rows();
        let row_kind = *element_at(&rows, row)?;
        let mtm = self.mtm();
        let style_sheet = self.style_sheet();
        let worklist = self.worklist();
        match row_kind {
            Row::Section(section) => {
                let identifier = NSString::from_str("taskSection");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| downcast::<TaskSectionRowView>(&view))
                    .unwrap_or_else(|| TaskSectionRowView::new(&identifier, mtm));
                let model = &worklist.sections[section as usize];
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                cell.set_on_toggle(Some(Rc::new(move || {
                    let Some(this) = weak.load() else { return };
                    let key = this.section_key(section);
                    let collapsed = this.ivars().collapsed_sections.borrow().contains(&key);
                    if collapsed {
                        this.ivars().collapsed_sections.borrow_mut().remove(&key);
                    } else {
                        this.ivars().collapsed_sections.borrow_mut().insert(key);
                    }
                    this.rebuild_rows(true);
                })));
                let collapsed = self.ivars().collapsed_sections.borrow().contains(&self.section_key(section));
                cell.configure(&model.title, model.open_count, collapsed, style_sheet);
                Some(Retained::into_super(Retained::into_super(cell)))
            }
            Row::Pile(section) => {
                let identifier = NSString::from_str("taskPile");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| downcast::<TaskPileRowView>(&view))
                    .unwrap_or_else(|| TaskPileRowView::new(&identifier, mtm));
                let key = self.section_key(section);
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                cell.set_on_toggle(Some(Rc::new(move || {
                    let Some(this) = weak.load() else { return };
                    let expanded = this.ivars().expanded_piles.borrow().contains(&key);
                    if expanded {
                        this.ivars().expanded_piles.borrow_mut().remove(&key);
                    } else {
                        this.ivars().expanded_piles.borrow_mut().insert(key);
                    }
                    this.rebuild_rows(true);
                })));
                let expanded = self.ivars().expanded_piles.borrow().contains(&key);
                cell.configure(worklist.sections[section as usize].done_count, expanded, style_sheet);
                Some(Retained::into_super(Retained::into_super(cell)))
            }
            Row::Add(section) => {
                let identifier = NSString::from_str("taskAdd");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| downcast::<TaskAddRowView>(&view))
                    .unwrap_or_else(|| TaskAddRowView::new(&identifier, mtm));
                // `None`, not `section`: the row's own index is only what the
                // last rebuild resolved, and the selection it was derived from
                // may have moved since.  `begin_new_task` re-resolves it.
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                cell.set_on_begin_edit(Some(Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.begin_new_task(None);
                    }
                })));
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                cell.set_on_commit(Some(Rc::new(move |text: &str| {
                    if let Some(this) = weak.load() {
                        this.commit_new_task(text);
                    }
                })));
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                cell.set_on_cancel(Some(Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.cancel_new_task();
                    }
                })));
                cell.configure(self.ivars().editing_add_section.get() == Some(section), style_sheet);
                Some(Retained::into_super(Retained::into_super(cell)))
            }
            Row::Task(index) => {
                let tasks = self.tasks();
                if !(index < tasks.len() as isize) {
                    return None;
                }
                let identifier = NSString::from_str("taskRow");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| downcast::<TaskRowView>(&view))
                    .unwrap_or_else(|| TaskRowView::new(&identifier, mtm));
                let weak: ObjcWeak<TaskPanelView> = ObjcWeak::from(self);
                cell.set_on_toggle(Some(Rc::new(move |task: &TaskItem| {
                    if let Some(this) = weak.load() {
                        this.toggle_task(task);
                    }
                })));
                let task = &tasks[index as usize];
                let section_title = worklist
                    .sections
                    .iter()
                    .find(|section| section.heading_index == task.heading_index)
                    .map_or("Document", |section| section.title.as_str());
                let is_selected = self.ivars().table.selectedRow() == row;
                cell.configure(task, section_title, is_selected, style_sheet);
                Some(Retained::into_super(Retained::into_super(cell)))
            }
        }
    }

    fn height_of_row(&self, table_view: &NSTableView, row: isize) -> CGFloat {
        let rows = self.rows();
        if !(row >= 0 && (row as usize) < rows.len()) {
            return TaskRowMetrics::MINIMUM_HEIGHT;
        }
        self.row_height(&rows, row, self.measurement_width(table_view.bounds().width()))
    }

    fn row_height(&self, rows: &[Row], row: isize, width: CGFloat) -> CGFloat {
        let Some(&row_kind) = element_at(rows, row) else { return TaskRowMetrics::MINIMUM_HEIGHT };
        match row_kind {
            Row::Section(_) => TaskRowMetrics::GROUP_HEIGHT,
            Row::Pile(_) => TaskRowMetrics::GROUP_HEIGHT,
            Row::Add(_) => TaskRowMetrics::MINIMUM_HEIGHT,
            Row::Task(index) => {
                if !(index < self.ivars().tasks.borrow().len() as isize) {
                    return TaskRowMetrics::MINIMUM_HEIGHT;
                }
                self.task_height(index, width)
            }
        }
    }

    /// Only the two rows whose selection actually changed are rebuilt.
    fn selection_did_change(&self) {
        let table = &self.ivars().table;
        let affected = NSMutableIndexSet::new();
        if let Some(previous) = self.ivars().last_selected_row.get()
            && previous < table.numberOfRows()
        {
            affected.addIndex(previous as usize);
        }
        let selected = table.selectedRow();
        if selected >= 0 {
            affected.addIndex(selected as usize);
        }
        self.ivars().last_selected_row.set(if selected >= 0 { Some(selected) } else { None });
        if affected.count() == 0 {
            return;
        }
        table.reloadDataForRowIndexes_columnIndexes(&affected, &NSIndexSet::indexSetWithIndex(0));
    }

    // MARK: Drag reorder

    fn pasteboard_writer_for_row(&self, row: isize) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
        let rows = self.rows();
        let Some(Row::Task(index)) = element_at(&rows, row).copied() else { return None };
        let item = NSPasteboardItem::new();
        item.setString_forType(&ns_string(&index.to_string()), unsafe { NSPasteboardTypeString });
        Some(ProtocolObject::from_retained(item))
    }

    /// `Int(info.draggingPasteboard.string(forType: .string) ?? "")`.
    fn dragged_source(info: &ProtocolObject<dyn NSDraggingInfo>) -> Option<isize> {
        let pasteboard = info.draggingPasteboard();
        let string = pasteboard.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string());
        string.unwrap_or_default().parse::<isize>().ok()
    }

    fn validate_drop(&self, table_view: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: isize) -> NSDragOperation {
        let Some(source) = Self::dragged_source(info) else { return NSDragOperation::empty() };
        if !(source < self.tasks().len() as isize) || self.drop_before_target(row, source).is_none() {
            return NSDragOperation::empty();
        }
        table_view.setDropRow_dropOperation(row, NSTableViewDropOperation::Above);
        NSDragOperation::Move
    }

    fn accept_drop(&self, info: &ProtocolObject<dyn NSDraggingInfo>, row: isize) -> bool {
        let Some(source) = Self::dragged_source(info) else { return false };
        if !(source < self.tasks().len() as isize) {
            return false;
        }
        let Some(target) = self.drop_before_target(row, source) else { return false };
        if let Some(delegate) = self.delegate() {
            match target {
                DropTarget::Before(index) => delegate.task_panel_did_move_task(self, source, Some(index)),
                DropTarget::EndOfGroup => delegate.task_panel_did_move_task(self, source, None),
            }
        }
        true
    }
}

// MARK: - Swift's `difference(from:)`

/// `new.difference(from: old)` for an `Equatable` element: the standard
/// library's `_myers(from:to:using:)` (Myers' O(ND) descent over a trace of
/// `_V` rows, then the backtrack that forms the changes).  Returns the
/// removal offsets (into `old`) and the insertion offsets (into `new`), which
/// is all `rebuildRows` reads.
fn collection_difference<T: PartialEq>(old: &[T], new: &[T]) -> (Vec<usize>, Vec<usize>) {
    /// `_V`: the rows of the triangular matrix, negative indexes interleaved
    /// with positive ones.
    struct V {
        a: Vec<isize>,
    }

    impl V {
        fn new(max_index: isize) -> V {
            V { a: vec![0; (max_index + 1) as usize] }
        }

        fn transform(index: isize) -> usize {
            (if index <= 0 { -index } else { index - 1 }) as usize
        }

        fn get(&self, index: isize) -> isize {
            self.a[Self::transform(index)]
        }

        fn set(&mut self, index: isize, value: isize) {
            self.a[Self::transform(index)] = value;
        }
    }

    let n = old.len() as isize;
    let m = new.len() as isize;
    let max = n + m;

    // _descent
    let mut trace: Vec<V> = Vec::new();
    let mut v = V::new(1);
    v.set(1, 0);
    let mut x: isize = 0;
    let mut y: isize = 0;
    'iterator: for d in 0..=max {
        trace.push(std::mem::replace(&mut v, V::new(d)));
        let prev_v = trace.last().expect("pushed");
        let mut k = -d;
        while k <= d {
            if k == -d {
                x = prev_v.get(k + 1);
            } else {
                let km = prev_v.get(k - 1);
                if k != d {
                    let kp = prev_v.get(k + 1);
                    x = if km < kp { kp } else { km + 1 };
                } else {
                    x = km + 1;
                }
            }
            y = x - k;
            while x < n && y < m {
                if old[x as usize] != new[y as usize] {
                    break;
                }
                x += 1;
                y += 1;
            }
            v.set(k, x);
            if x >= n && y >= m {
                break 'iterator;
            }
            k += 2;
        }
        if x >= n && y >= m {
            break;
        }
    }

    // _formChanges
    let mut removals = Vec::new();
    let mut insertions = Vec::new();
    let mut x = n;
    let mut y = m;
    let mut d = trace.len() as isize - 1;
    while d > 0 {
        let v = &trace[d as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && v.get(k - 1) < v.get(k + 1)) { k + 1 } else { k - 1 };
        let prev_x = v.get(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            // No change at this position.
            x -= 1;
            y -= 1;
        }
        if y != prev_y {
            insertions.push(prev_y as usize);
        } else {
            removals.push(prev_x as usize);
        }
        x = prev_x;
        y = prev_y;
        d -= 1;
    }
    (removals, insertions)
}

// MARK: - Row geometry

/// The one place the panel's geometry is written down (§8.5).
pub struct TaskRowMetrics;

impl TaskRowMetrics {
    /// One left rail for the whole panel.
    pub const CONTENT_INSET: CGFloat = 18.0;
    pub const CHECKBOX_INSET: CGFloat = Self::CONTENT_INSET;
    pub const BOX_SIDE: CGFloat = 17.0;
    /// A child's box begins exactly where its parent's box ended.
    pub const INDENT_STEP: CGFloat = Self::BOX_SIDE;
    pub const MAXIMUM_INDENT: isize = 4;
    /// Checkbox → label gap, label → chevron gap, chevron, chevron → edge.
    pub const LABEL_GAP: CGFloat = 12.0;
    pub const GLYPH_GAP: CGFloat = 6.0;
    pub const GLYPH_WIDTH: CGFloat = 8.0;
    pub const GLYPH_INSET: CGFloat = 10.0;
    /// Air above and below a single line of task text.
    pub const VERTICAL_PADDING: CGFloat = 11.0;
    pub const MINIMUM_HEIGHT: CGFloat = 38.0;
    /// Section headers and pile rows carry their air above the label.
    pub const GROUP_HEIGHT: CGFloat = 26.0;
    pub const MINIMUM_LABEL_WIDTH: CGFloat = 90.0;
    pub const MAXIMUM_LINES: isize = 3;

    pub fn leading(indent_level: isize) -> CGFloat {
        Self::CHECKBOX_INSET
            + indent_level.min(Self::MAXIMUM_INDENT) as CGFloat * Self::INDENT_STEP
            + Self::BOX_SIDE
            + Self::LABEL_GAP
    }

    pub fn trailing_chrome() -> CGFloat {
        Self::GLYPH_GAP + Self::GLYPH_WIDTH + Self::GLYPH_INSET
    }

    /// Top of the checkbox, placed so its centre lands on the first line's
    /// x-height centre.
    pub fn first_line_box_top() -> CGFloat {
        let font = PanelFont::task_row();
        let optical_centre = Self::VERTICAL_PADDING + font.ascender() - font.xHeight() / 2.0;
        (optical_centre - Self::BOX_SIDE / 2.0).round()
    }

    /// Centre of the box one level shallower — where a nesting rail hangs
    /// from.
    pub fn rail_x(level: isize) -> CGFloat {
        Self::CHECKBOX_INSET + (level - 1) as CGFloat * Self::INDENT_STEP + Self::BOX_SIDE / 2.0
    }
}

// MARK: - Row surface

pub struct TaskRowSurfaceViewIvars {
    /// The row's own surface, drawn in the same rect the selection uses.
    surface_layer: Retained<CALayer>,
    is_hovered: Cell<bool>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
}

/// The arguments of `TaskRowSurfaceView.init(identifier:ownsBackgroundFade:)`,
/// staged for the base's `initWithFrame:` (see `MessageBarInit`).
struct TaskRowSurfaceInit {
    identifier: Retained<NSString>,
    owns_background_fade: bool,
}

thread_local! {
    static PENDING_ROW_SURFACE: RefCell<Option<TaskRowSurfaceInit>> = const { RefCell::new(None) };
}

define_class!(
    /// The chrome every task-panel row shares: the rounded surface drawn
    /// behind the row and the pointer tracking that drives it.
    // SAFETY: `initWithFrame:` sets the ivars from the staged init arguments;
    // it is only reached through a subclass's `init(identifier:)`.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskRowSurfaceView"]
    #[ivars = TaskRowSurfaceViewIvars]
    struct TaskRowSurfaceView;

    unsafe impl NSObjectProtocol for TaskRowSurfaceView {}

    impl TaskRowSurfaceView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let init = PENDING_ROW_SURFACE
                .with(|pending| pending.borrow_mut().take())
                .expect("use of unimplemented initializer 'init(frame:)' for class 'TaskRowSurfaceView'");
            let this = this.set_ivars(TaskRowSurfaceViewIvars {
                surface_layer: CALayer::new(),
                is_hovered: Cell::new(false),
                tracking_area: RefCell::new(None),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.setIdentifier(Some(&init.identifier));
            this.setWantsLayer(true);
            let surface_layer = &this.ivars().surface_layer;
            surface_layer.setCornerRadius(PanelMetrics::ROW_SURFACE_RADIUS);
            if init.owns_background_fade {
                null_actions(surface_layer, &["position", "bounds", "backgroundColor"]);
            } else {
                null_actions(surface_layer, &["position", "bounds"]);
            }
            if let Some(layer) = this.layer() {
                layer.addSublayer(surface_layer);
            }
            this
        }

        /// Subclasses restyle here; the base owns only the flag and the
        /// tracking.
        #[unsafe(method(hoverDidChange))]
        fn __hover_did_change(&self) {}

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            without_actions(|| self.ivars().surface_layer.setFrame(PanelMetrics::row_surface(self.bounds())));
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.set_hovered(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.set_hovered(false);
        }
    }
);

impl TaskRowSurfaceView {
    /// Stages `init(identifier:ownsBackgroundFade:)`'s arguments for a
    /// subclass about to send `initWithFrame:` to `super`.
    fn stage_init(identifier: &NSString, owns_background_fade: bool) {
        PENDING_ROW_SURFACE.with(|pending| {
            *pending.borrow_mut() = Some(TaskRowSurfaceInit { identifier: identifier.copy(), owns_background_fade })
        });
    }

    fn surface_layer(&self) -> &CALayer {
        &self.ivars().surface_layer
    }

    fn is_hovered(&self) -> bool {
        self.ivars().is_hovered.get()
    }

    /// `isHovered` with its `didSet` (dispatched, so the subclass's
    /// `hoverDidChange` runs).
    fn set_hovered(&self, hovered: bool) {
        self.ivars().is_hovered.set(hovered);
        let _: () = unsafe { msg_send![self, hoverDidChange] };
    }
}

/// `surfaceLayer.presentation()?.backgroundColor ?? surfaceLayer.backgroundColor`.
fn presentation_background(layer: &CALayer) -> Option<Retained<objc2_core_graphics::CGColor>> {
    use super::appkit_support::Presentation;
    layer.__presentation().and_then(|presentation| presentation.backgroundColor()).or_else(|| layer.backgroundColor())
}

/// A quick `backgroundColor` fade to `color`, keyed "surface".
fn fade_surface(surface_layer: &CALayer, color: &objc2_core_graphics::CGColor) {
    let fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("backgroundColor")));
    set_color_values(&fade, presentation_background(surface_layer).as_deref(), color);
    fade.setDuration(motion::QUICK);
    fade.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
    surface_layer.addAnimation_forKey(&fade, Some(&NSString::from_str("surface")));
    surface_layer.setBackgroundColor(Some(color));
}

/// The non-animated surface change: no implicit action, no fade in flight.
fn set_surface(surface_layer: &CALayer, color: &objc2_core_graphics::CGColor) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    surface_layer.removeAnimationForKey(&NSString::from_str("surface"));
    surface_layer.setBackgroundColor(Some(color));
    CATransaction::commit();
}

/// `CGColor.clear`.
fn clear_cg_color() -> Retained<objc2_core_graphics::CGColor> {
    objc2_core_graphics::CGColor::constant_color(Some(unsafe { objc2_core_graphics::kCGColorClear }))
        .expect("kCGColorClear")
        .into()
}

// MARK: - Task row

pub struct TaskRowViewIvars {
    on_toggle: RefCell<Option<Rc<dyn Fn(&TaskItem)>>>,
    checkbox: Retained<PanelCheckbox>,
    label: Retained<NSTextField>,
    jump_glyph: Retained<NSImageView>,
    /// The completion strike, drawn only while the moment plays.
    strike_layer: Retained<CALayer>,
    checkbox_leading: RefCell<Option<Retained<NSLayoutConstraint>>>,
    task: RefCell<TaskItem>,
    indent_level: Cell<isize>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    is_pressed: Cell<bool>,
    is_selected: Cell<bool>,
}

define_class!(
    /// A worklist row: the document's checkbox at panel size, the task's
    /// text, and a jump chevron on hover.
    // SAFETY: `initWithFrame:` is forwarded to `TaskRowSurfaceView` in `new`
    // after the ivars are set and the base's arguments are staged.
    #[unsafe(super(TaskRowSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskRowView"]
    #[ivars = TaskRowViewIvars]
    struct TaskRowView;

    unsafe impl NSObjectProtocol for TaskRowView {}

    impl TaskRowView {
        #[unsafe(method(hoverDidChange))]
        fn __hover_did_change(&self) {
            self.update_surface(true);
        }

        /// The nesting rails: each ancestor box leaves a one-point rule
        /// centred on where it sat.
        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            let indent_level = self.ivars().indent_level.get();
            if !(indent_level > 0) {
                return;
            }
            let style_sheet = self.ivars().style_sheet.borrow().clone();
            let contrast = style_sheet.increase_contrast;
            style_sheet.text.panel_alpha(if contrast { 0.16 } else { 0.09 }, contrast).setFill();
            for level in 1..=indent_level {
                let x = TaskRowMetrics::rail_x(level) - 0.5;
                rect_fill(rect(x, 0.0, 1.0, self.bounds().height()));
            }
        }
    }
);

impl TaskRowView {
    /// The height the panel's `heightOfRow` reports, measured from the same
    /// metrics the live row constrains from.
    pub fn fitting_height(task: &TaskItem, width: CGFloat, style_sheet: &StyleSheet) -> CGFloat {
        let _ = style_sheet;
        let indent = task.indent_level.min(TaskRowMetrics::MAXIMUM_INDENT);
        let text_width = smax(
            TaskRowMetrics::MINIMUM_LABEL_WIDTH,
            width - TaskRowMetrics::leading(indent) - TaskRowMetrics::trailing_chrome(),
        );
        let attributes = attributes_dictionary(&[(keys::font(), &*PanelFont::task_row())]);
        let rect = string_bounding_rect(&task.text, NSSize::new(text_width, CGFloat::MAX), &attributes);
        let font = PanelFont::task_row();
        let line_height = (font.ascender() - font.descender() + font.leading()).ceil();
        let capped = smin(rect.size.height.ceil(), line_height * TaskRowMetrics::MAXIMUM_LINES as CGFloat);
        smax(TaskRowMetrics::MINIMUM_HEIGHT, TaskRowMetrics::VERTICAL_PADDING * 2.0 + capped)
    }

    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<TaskRowView> {
        let checkbox = PanelCheckbox::new(TaskRowMetrics::BOX_SIDE, 0.5, mtm);
        let label = label("", mtm);
        let jump_glyph = NSImageView::new(mtm);
        let strike_layer = CALayer::new();
        let task = TaskItem::new(false, SourceRange::new(0, 1), SourceRange::new(0, 0), "", None, 0);
        let this = Self::alloc(mtm).set_ivars(TaskRowViewIvars {
            on_toggle: RefCell::new(None),
            checkbox,
            label,
            jump_glyph,
            strike_layer,
            checkbox_leading: RefCell::new(None),
            task: RefCell::new(task),
            indent_level: Cell::new(0),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
            is_pressed: Cell::new(false),
            is_selected: Cell::new(false),
        });
        TaskRowSurfaceView::stage_init(identifier, true);
        let this: Retained<TaskRowView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        ivars.strike_layer.setZPosition(10.0);
        ivars.strike_layer.setOpacity(0.0);
        null_actions(&ivars.strike_layer, &["position", "bounds"]);
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ivars.strike_layer);
        }

        let checkbox = &ivars.checkbox;
        checkbox.setTranslatesAutoresizingMaskIntoConstraints(false);
        let weak: ObjcWeak<TaskRowView> = ObjcWeak::from(&*this);
        checkbox.set_on_toggle(Some(Rc::new(move || {
            let Some(this) = weak.load() else { return };
            let is_checked = this.ivars().task.borrow().is_checked;
            if !is_checked {
                this.play_completion_moment();
            }
            this.pulse_row_glow();
            let handler = this.ivars().on_toggle.borrow().clone();
            let task = this.ivars().task.borrow().clone();
            if let Some(handler) = handler {
                handler(&task);
            }
        })));
        let weak: ObjcWeak<TaskRowView> = ObjcWeak::from(&*this);
        checkbox.set_on_press_change(Some(Rc::new(move |pressed| {
            if let Some(this) = weak.load() {
                this.set_pressed(pressed);
            }
        })));
        this.addSubview(checkbox);

        let label = &ivars.label;
        label.setFont(Some(&PanelFont::task_row()));
        label.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        label.setMaximumNumberOfLines(TaskRowMetrics::MAXIMUM_LINES);
        if let Some(cell) = label.cell() {
            cell.setWraps(true);
            cell.setScrollable(false);
        }
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(label);

        let jump_glyph = &ivars.jump_glyph;
        jump_glyph.setImage(system_symbol("chevron.right", Some("Jump to task")).as_deref());
        jump_glyph.setContentTintColor(Some(&NSColor::clearColor()));
        jump_glyph.setSymbolConfiguration(Some(&symbol_configuration(9.0, weight_semibold())));
        jump_glyph.setTranslatesAutoresizingMaskIntoConstraints(false);
        jump_glyph.setAlphaValue(0.0);
        this.addSubview(jump_glyph);

        let checkbox_leading =
            checkbox.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), TaskRowMetrics::CHECKBOX_INSET);
        *ivars.checkbox_leading.borrow_mut() = Some(checkbox_leading.clone());
        activate(&[
            checkbox_leading,
            // The box centres on the first line's x-height, not on the row.
            checkbox
                .topAnchor()
                .constraintEqualToAnchor_constant(&this.topAnchor(), TaskRowMetrics::first_line_box_top()),
            checkbox.widthAnchor().constraintEqualToConstant(TaskRowMetrics::BOX_SIDE),
            checkbox.heightAnchor().constraintEqualToConstant(TaskRowMetrics::BOX_SIDE),
            label.leadingAnchor().constraintEqualToAnchor_constant(&checkbox.trailingAnchor(), TaskRowMetrics::LABEL_GAP),
            label.trailingAnchor().constraintEqualToAnchor_constant(&jump_glyph.leadingAnchor(), -TaskRowMetrics::GLYPH_GAP),
            label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), TaskRowMetrics::VERTICAL_PADDING),
            jump_glyph.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -TaskRowMetrics::GLYPH_INSET),
            jump_glyph.centerYAnchor().constraintEqualToAnchor(&checkbox.centerYAnchor()),
            jump_glyph.widthAnchor().constraintEqualToConstant(TaskRowMetrics::GLYPH_WIDTH),
        ]);
        this
    }

    fn set_on_toggle(&self, handler: Option<Rc<dyn Fn(&TaskItem)>>) {
        *self.ivars().on_toggle.borrow_mut() = handler;
    }

    /// `isPressed` with its `didSet`.
    fn set_pressed(&self, pressed: bool) {
        self.ivars().is_pressed.set(pressed);
        self.update_surface(true);
    }

    fn configure(&self, task: &TaskItem, section_title: &str, is_selected: bool, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.task.borrow_mut() = task.clone();
        let indent_level = task.indent_level.min(TaskRowMetrics::MAXIMUM_INDENT);
        ivars.indent_level.set(indent_level);
        *ivars.style_sheet.borrow_mut() = style_sheet.clone();
        ivars.is_selected.set(is_selected);
        let leading = ivars.checkbox_leading.borrow().clone();
        if let Some(leading) = leading {
            leading.setConstant(TaskRowMetrics::CHECKBOX_INSET + indent_level as CGFloat * TaskRowMetrics::INDENT_STEP);
        }
        // A task that still had to be clipped says the rest on hover rather
        // than losing it (§8.5).
        let text = swift_text::trim_whitespaces(&task.text);
        self.setToolTip(Some(&ns_string(text)));

        let checkbox = &ivars.checkbox;
        checkbox.set_style_sheet(style_sheet.clone());
        checkbox.set_checked(task.is_checked, false);
        set_label(
            &**checkbox,
            &if task.is_checked {
                format!("Mark incomplete: {}", task.text)
            } else {
                format!("Mark complete: {}", task.text)
            },
        );
        set_value(&**checkbox, if task.is_checked { "Completed" } else { "Incomplete" });

        ivars.jump_glyph.setContentTintColor(Some(&style_sheet.text_faint));

        // Both states keep full-primary text; the filled circle carries done.
        // The strike exists only as the completion moment.
        ivars.label.setAttributedStringValue(&attributed_string(
            text,
            &[(keys::font(), &*PanelFont::task_row()), (keys::foreground_color(), &*style_sheet.text)],
        ));
        set_role(self, role::row());
        set_label(
            self,
            &format!("{} task: {text}, in {section_title}", if task.is_checked { "Completed" } else { "Incomplete" }),
        );
        // Reuse: a row handed back to the pool must not keep a half-run
        // completion sweep from a previous life.
        self.set_pressed(false);
        ivars.strike_layer.removeAllAnimations();
        ivars.strike_layer.setOpacity(0.0);
        if let Some(layer) = self.layer() {
            layer.setTransform(unsafe { CATransform3DIdentity });
        }
        self.setAlphaValue(1.0);
        self.setNeedsDisplay(true);
        self.update_surface(false);
        self.update_glyph(false);
    }

    /// The completion moment: the strike draws itself across the first line
    /// while the label cools to its done colour.
    fn play_completion_moment(&self) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        if !(!style_sheet.reduce_motion && self.window().is_some()) {
            return;
        }
        self.layoutSubtreeIfNeeded();
        let font = PanelFont::task_row();
        let label_frame = ivars.label.frame();
        let strike_y = label_frame.min_y() + font.ascender() - font.xHeight() / 2.0;
        let strike_layer = &ivars.strike_layer;
        strike_layer.removeAllAnimations();
        strike_layer.setBackgroundColor(Some(&cg(&style_sheet.text_faint)));
        strike_layer.setOpacity(1.0);
        strike_layer.setFrame(rect(label_frame.min_x(), strike_y, 0.0, 1.0));
        CATransaction::begin();
        CATransaction::setAnimationDuration(motion::STANDARD);
        CATransaction::setAnimationTimingFunction(Some(&motion::timing(Curve::Decelerate)));
        strike_layer.setFrame(rect(label_frame.min_x(), strike_y, label_frame.width(), 1.0));
        CATransaction::commit();
        ivars.label.setTextColor(Some(&style_sheet.text_secondary));
    }

    /// Hover, press, and the completion glow are the same rectangle at three
    /// strengths.
    fn surface_color(&self) -> Retained<NSColor> {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        if ivars.is_pressed.get() {
            return style_sheet.selection.colorWithAlphaComponent(if contrast { 0.95 } else { 0.75 });
        }
        if self.is_hovered() {
            return style_sheet.selection.colorWithAlphaComponent(if contrast { 0.8 } else { 0.55 });
        }
        NSColor::clearColor()
    }

    fn update_surface(&self, animated: bool) {
        let color = cg(&self.surface_color());
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        if !(animated && !reduce_motion && self.window().is_some()) {
            set_surface(self.surface_layer(), &color);
            self.update_glyph(false);
            return;
        }
        fade_surface(self.surface_layer(), &color);
        self.update_glyph(true);
    }

    fn update_glyph(&self, animated: bool) {
        let ivars = self.ivars();
        let target: CGFloat = if self.is_hovered() || ivars.is_selected.get() { 1.0 } else { 0.0 };
        let reduce_motion = ivars.style_sheet.borrow().reduce_motion;
        if !(animated && !reduce_motion) {
            ivars.jump_glyph.setAlphaValue(target);
            return;
        }
        if ivars.jump_glyph.alphaValue() == target {
            return;
        }
        let jump_glyph = ivars.jump_glyph.clone();
        motion::run(
            reduce_motion,
            motion::QUICK,
            Curve::Decelerate,
            move |_| jump_glyph.animator().setAlphaValue(target),
            None,
        );
    }

    /// The row answers a completed task with one brief accent wash in the
    /// checkbox's own colour, which settles back to whatever the row was
    /// already showing.
    fn pulse_row_glow(&self) {
        let style_sheet = self.ivars().style_sheet.borrow().clone();
        if !(!style_sheet.reduce_motion && self.window().is_some()) {
            return;
        }
        let contrast = style_sheet.increase_contrast;
        let glow = style_sheet.accent.panel_alpha(if contrast { 0.18 } else { 0.12 }, contrast);
        let settle = self.surface_color();
        let wash = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("backgroundColor")));
        let values = cg_array(&[cg(&glow), cg(&glow), cg(&settle)]);
        let key_times = NSArray::from_retained_slice(&[
            NSNumber::new_f64(0.0),
            NSNumber::new_f64(0.25),
            NSNumber::new_f64(1.0),
        ]);
        let timing_functions: Retained<NSArray<CAMediaTimingFunction>> =
            NSArray::from_retained_slice(&[motion::timing(Curve::EaseOut), motion::timing(Curve::EaseOut)]);
        unsafe {
            wash.setValues(Some(&values));
            wash.setKeyTimes(Some(&key_times));
        }
        wash.setDuration(motion::DELIBERATE);
        wash.setTimingFunctions(Some(&timing_functions));
        let surface_layer = self.surface_layer();
        surface_layer.addAnimation_forKey(&wash, Some(&NSString::from_str("surface")));
        surface_layer.setBackgroundColor(Some(&cg(&settle)));
    }
}

// MARK: - Section header row

/// The worklist's disclosure glyph, pre-rendered pointing right and down.
/// Swapping two pre-rendered images keeps every frame upright and exact; the
/// square canvases share a size, so the swap never nudges the title.
struct TaskDisclosureGlyph;

thread_local! {
    static DISCLOSURE_RIGHT: OnceCell<Option<Retained<NSImage>>> = const { OnceCell::new() };
    static DISCLOSURE_DOWN: OnceCell<Option<Retained<NSImage>>> = const { OnceCell::new() };
}

impl TaskDisclosureGlyph {
    /// `TaskDisclosureGlyph.right` (a lazy `static let`).
    fn right() -> Option<Retained<NSImage>> {
        DISCLOSURE_RIGHT.with(|cell| cell.get_or_init(|| Self::make(false)).clone())
    }

    /// `TaskDisclosureGlyph.down` (a lazy `static let`).
    fn down() -> Option<Retained<NSImage>> {
        DISCLOSURE_DOWN.with(|cell| cell.get_or_init(|| Self::make(true)).clone())
    }

    fn make(pointing_down: bool) -> Option<Retained<NSImage>> {
        let configuration = symbol_configuration(8.0, weight_bold());
        let source = configured_symbol("chevron.right", None, &configuration)?;
        if !pointing_down {
            return Some(source);
        }
        let glyph = source.size();
        // A square canvas, so the down glyph's extents match the right one's
        // slot and the turn never resizes anything.
        let side = smax(glyph.width, glyph.height);
        let handler = RcBlock::new(move |rect: NSRect| -> Bool {
            let transform = NSAffineTransform::new();
            transform.translateXBy_yBy(rect.mid_x(), rect.mid_y());
            // AppKit's y-up space: a clockwise quarter-turn of the
            // right-chevron points it down.
            transform.rotateByDegrees(-90.0);
            transform.translateXBy_yBy(-glyph.width / 2.0, -glyph.height / 2.0);
            objc2_app_kit::NSAffineTransformNSAppKitAdditions::concat(&*transform);
            source.drawAtPoint_fromRect_operation_fraction(NSPoint::new(0.0, 0.0), RECT_ZERO, NSCompositingOperation::SourceOver, 1.0);
            Bool::YES
        });
        let rotated = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(side, side), false, &handler);
        // Drawn bitmaps aren't templates by default; the rows tint their
        // chevron through `contentTintColor`, which only templates honour.
        rotated.setTemplate(true);
        Some(rotated)
    }
}

/// `chevron.image != image` (Swift's `!=` on `NSImage?`, which calls
/// `isEqual:`).
fn image_differs(current: Option<&NSImage>, image: Option<&NSImage>) -> bool {
    match (current, image) {
        (None, None) => false,
        (Some(current), Some(image)) => !current.isEqual(Some(image)),
        _ => true,
    }
}

/// `setChevron(open:animated:)`, shared by the section and pile rows.
fn set_disclosure_chevron(chevron: &NSImageView, open: bool, animated: bool, reduce_motion: bool, has_window: bool) {
    let image = if open { TaskDisclosureGlyph::down() } else { TaskDisclosureGlyph::right() };
    if !image_differs(chevron.image().as_deref(), image.as_deref()) {
        return;
    }
    if animated
        && !reduce_motion
        && has_window
        && let Some(layer) = chevron.layer()
    {
        let fade = CATransition::animation();
        fade.setType(unsafe { kCATransitionFade });
        fade.setDuration(motion::QUICK);
        fade.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
        layer.addAnimation_forKey(&fade, Some(&NSString::from_str("disclosure")));
    }
    chevron.setImage(image.as_deref());
}

pub struct TaskSectionRowViewIvars {
    on_toggle: Handler,
    chevron: Retained<NSImageView>,
    title_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
    style_sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    /// A section's own row: disclosure chevron, the heading's title, and how
    /// much of it is left.
    // SAFETY: `initWithFrame:` is forwarded to `TaskRowSurfaceView` in `new`
    // after the ivars are set and the base's arguments are staged.
    #[unsafe(super(TaskRowSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskSectionRowView"]
    #[ivars = TaskSectionRowViewIvars]
    struct TaskSectionRowView;

    unsafe impl NSObjectProtocol for TaskSectionRowView {}

    impl TaskSectionRowView {
        #[unsafe(method(hoverDidChange))]
        fn __hover_did_change(&self) {
            self.update_surface();
        }

        /// Claiming the press keeps the table's own tracking from swallowing
        /// the release, so the click pair always lands on the row.
        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            if !self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None)) {
                return;
            }
            let handler = self.ivars().on_toggle.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
        }
    }
);

impl TaskSectionRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<TaskSectionRowView> {
        let this = Self::alloc(mtm).set_ivars(TaskSectionRowViewIvars {
            on_toggle: RefCell::new(None),
            chevron: NSImageView::new(mtm),
            title_label: label("", mtm),
            status_label: label("", mtm),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
        });
        TaskRowSurfaceView::stage_init(identifier, true);
        let this: Retained<TaskSectionRowView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        let chevron = &ivars.chevron;
        chevron.setSymbolConfiguration(Some(&symbol_configuration(8.0, weight_bold())));
        // The symbol draws at its configured point size, centred — never
        // scaled to fill the square, so the glyph stays pixel-crisp.
        chevron.setImageScaling(NSImageScaling::ScaleNone);
        chevron.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(chevron);

        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::group()));
        title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(title_label);

        let status_label = &ivars.status_label;
        status_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            PanelFont::secondary().pointSize(),
            weight_regular(),
        )));
        status_label.setAlignment(NSTextAlignment::Right);
        status_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(status_label);

        let rail = TaskRowMetrics::CONTENT_INSET;
        activate(&[
            chevron.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), rail),
            chevron.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            // A fixed square slot: the glyph is always centred in the same
            // box, so swapping the right/down drawings never moves the title.
            chevron.widthAnchor().constraintEqualToConstant(10.0),
            chevron.heightAnchor().constraintEqualToConstant(10.0),
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&chevron.trailingAnchor(), 4.0),
            title_label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            status_label.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            status_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -rail),
            status_label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
        ]);
        this
    }

    fn set_on_toggle(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_toggle.borrow_mut() = handler;
    }

    fn configure(&self, title: &str, open_count: isize, collapsed: bool, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.style_sheet.borrow_mut() = style_sheet.clone();
        set_label(&*ivars.chevron, if collapsed { "Expand section" } else { "Collapse section" });
        ivars.chevron.setContentTintColor(Some(&style_sheet.text_faint));
        set_disclosure_chevron(&ivars.chevron, !collapsed, self.window().is_some(), style_sheet.reduce_motion, self.window().is_some());
        ivars.title_label.setStringValue(&ns_string(title));
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        // Status is status: one colour, whatever it says.
        ivars.status_label.setStringValue(&ns_string(&if open_count > 0 {
            format!("{open_count} left")
        } else {
            "All done".to_owned()
        }));
        ivars.status_label.setTextColor(Some(&style_sheet.text_faint));
        set_role(self, role::button());
        set_label(
            self,
            &format!("{title} section, {open_count} left, {}", if collapsed { "collapsed" } else { "expanded" }),
        );
        self.update_surface();
    }

    fn update_surface(&self) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        let hovered = self.is_hovered();
        let color = if hovered {
            cg(&style_sheet.text.panel_alpha(if contrast { 0.1 } else { 0.06 }, contrast))
        } else {
            clear_cg_color()
        };
        // Hover warms the words a step as well as the pill.
        ivars.title_label.setTextColor(Some(if hovered { &style_sheet.text } else { &style_sheet.text_secondary }));
        ivars
            .chevron
            .setContentTintColor(Some(if hovered { &style_sheet.text_secondary } else { &style_sheet.text_faint }));
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.surface_layer().setBackgroundColor(Some(&color));
        CATransaction::commit();
    }
}

// MARK: - Completed pile row

pub struct TaskPileRowViewIvars {
    on_toggle: Handler,
    chevron: Retained<NSImageView>,
    label: Retained<NSTextField>,
    style_sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    /// The "4 completed" row at a section's tail.
    // SAFETY: `initWithFrame:` is forwarded to `TaskRowSurfaceView` in `new`
    // after the ivars are set and the base's arguments are staged.
    #[unsafe(super(TaskRowSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskPileRowView"]
    #[ivars = TaskPileRowViewIvars]
    struct TaskPileRowView;

    unsafe impl NSObjectProtocol for TaskPileRowView {}

    impl TaskPileRowView {
        #[unsafe(method(hoverDidChange))]
        fn __hover_did_change(&self) {
            self.apply_style(true);
        }

        /// Claiming the press keeps the table's own tracking from swallowing
        /// the release, so the click pair always lands on the row.
        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            if !self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None)) {
                return;
            }
            let handler = self.ivars().on_toggle.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
        }
    }
);

impl TaskPileRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<TaskPileRowView> {
        let this = Self::alloc(mtm).set_ivars(TaskPileRowViewIvars {
            on_toggle: RefCell::new(None),
            chevron: NSImageView::new(mtm),
            label: label("", mtm),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
        });
        TaskRowSurfaceView::stage_init(identifier, true);
        let this: Retained<TaskPileRowView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        let chevron = &ivars.chevron;
        chevron.setSymbolConfiguration(Some(&symbol_configuration(8.0, weight_bold())));
        chevron.setImageScaling(NSImageScaling::ScaleNone);
        chevron.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(chevron);

        let label = &ivars.label;
        label.setFont(Some(&PanelFont::secondary()));
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(label);

        // Chevron centres on the checkbox column; the label starts on the
        // text column, so the pile reads as a property of the list.
        activate(&[
            chevron.centerXAnchor().constraintEqualToAnchor_constant(
                &this.leadingAnchor(),
                TaskRowMetrics::CHECKBOX_INSET + TaskRowMetrics::BOX_SIDE / 2.0,
            ),
            chevron.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            // The same fixed square slot the section row's chevron gets.
            chevron.widthAnchor().constraintEqualToConstant(10.0),
            chevron.heightAnchor().constraintEqualToConstant(10.0),
            label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), TaskRowMetrics::leading(0)),
            label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
        ]);
        this
    }

    fn set_on_toggle(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_toggle.borrow_mut() = handler;
    }

    fn configure(&self, count: isize, expanded: bool, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.style_sheet.borrow_mut() = style_sheet.clone();
        set_label(&*ivars.chevron, if expanded { "Hide completed tasks" } else { "Show completed tasks" });
        set_disclosure_chevron(&ivars.chevron, expanded, self.window().is_some(), style_sheet.reduce_motion, self.window().is_some());
        ivars.label.setStringValue(&ns_string(&if count == 1 {
            "1 completed".to_owned()
        } else {
            format!("{count} completed")
        }));
        set_role(self, role::button());
        set_label(
            self,
            &if expanded { format!("Hide {count} completed tasks") } else { format!("Show {count} completed tasks") },
        );
        self.apply_style(false);
    }

    fn apply_style(&self, animated: bool) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        let warm = self.is_hovered();
        ivars.label.setTextColor(Some(if warm { &style_sheet.text_secondary } else { &style_sheet.text_faint }));
        let chevron_tint = if warm {
            style_sheet.text_secondary.clone()
        } else {
            style_sheet.text.panel_alpha(if contrast { 0.5 } else { 0.35 }, contrast)
        };
        ivars.chevron.setContentTintColor(Some(&chevron_tint));
        let surface = if warm {
            cg(&style_sheet.text.panel_alpha(if contrast { 0.1 } else { 0.07 }, contrast))
        } else {
            clear_cg_color()
        };
        if !(animated && !style_sheet.reduce_motion && self.window().is_some()) {
            set_surface(self.surface_layer(), &surface);
            return;
        }
        fade_surface(self.surface_layer(), &surface);
    }
}

// MARK: - Quick-add row

pub struct TaskAddRowViewIvars {
    on_begin_edit: Handler,
    on_commit: RefCell<Option<Rc<dyn Fn(&str)>>>,
    on_cancel: Handler,
    plus_glyph: Retained<NSImageView>,
    hint_label: Retained<NSTextField>,
    text_field: Retained<NSTextField>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    editing: Cell<bool>,
    /// Esc marks the edit so `controlTextDidEndEditing` does not commit it.
    cancelled: Cell<bool>,
    /// Return and focus loss can arrive for the same field edit; the row
    /// owns one commit boundary.
    commit_sent: Cell<bool>,
}

define_class!(
    /// The "+ Add task" row, and the reason the panel is two-way.
    // SAFETY: `initWithFrame:` is forwarded to `TaskRowSurfaceView` in `new`
    // after the ivars are set and the base's arguments are staged.
    #[unsafe(super(TaskRowSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskAddRowView"]
    #[ivars = TaskAddRowViewIvars]
    struct TaskAddRowView;

    unsafe impl NSObjectProtocol for TaskAddRowView {}

    impl TaskAddRowView {
        #[unsafe(method(hoverDidChange))]
        fn __hover_did_change(&self) {
            self.apply_style(self.window().is_some());
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn __accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        /// Begin editing on press. NSTableView may recycle or select the row
        /// between down and up; waiting for release made Add task
        /// intermittent.
        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test(point)
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            if self.ivars().editing.get() {
                return;
            }
            let handler = self.ivars().on_begin_edit.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn __accessibility_perform_press(&self) -> bool {
            self.accessibility_perform_press()
        }
    }

    unsafe impl NSControlTextEditingDelegate for TaskAddRowView {
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn __do_command_by_selector(&self, _control: &NSControl, _text_view: &NSTextView, command: Sel) -> bool {
            self.do_command(command)
        }

        /// Clicking away commits whatever is typed.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn __control_text_did_end_editing(&self, _notification: &NSNotification) {
            self.commit_if_needed();
        }
    }

    unsafe impl NSTextFieldDelegate for TaskAddRowView {}
);

impl TaskAddRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<TaskAddRowView> {
        let this = Self::alloc(mtm).set_ivars(TaskAddRowViewIvars {
            on_begin_edit: RefCell::new(None),
            on_commit: RefCell::new(None),
            on_cancel: RefCell::new(None),
            plus_glyph: NSImageView::new(mtm),
            hint_label: label("Add task", mtm),
            text_field: NSTextField::new(mtm),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
            editing: Cell::new(false),
            cancelled: Cell::new(false),
            commit_sent: Cell::new(false),
        });
        TaskRowSurfaceView::stage_init(identifier, true);
        let this: Retained<TaskAddRowView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        let plus_glyph = &ivars.plus_glyph;
        plus_glyph.setImage(system_symbol("plus", Some("Add task")).as_deref());
        plus_glyph.setSymbolConfiguration(Some(&symbol_configuration(10.0, super::appkit_support::weight_medium())));
        plus_glyph.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(plus_glyph);

        let hint_label = &ivars.hint_label;
        hint_label.setFont(Some(&PanelFont::row()));
        hint_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(hint_label);

        let text_field = &ivars.text_field;
        text_field.setFont(Some(&PanelFont::row()));
        text_field.setBezeled(false);
        text_field.setDrawsBackground(false);
        text_field.setFocusRingType(NSFocusRingType::None);
        text_field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        text_field.setPlaceholderString(Some(&ns_string("New task")));
        // SAFETY: the field's delegate is weak; the row owns the field.
        unsafe { text_field.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        text_field.setTranslatesAutoresizingMaskIntoConstraints(false);
        text_field.setHidden(true);
        this.addSubview(text_field);

        activate(&[
            plus_glyph.centerXAnchor().constraintEqualToAnchor_constant(
                &this.leadingAnchor(),
                TaskRowMetrics::CHECKBOX_INSET + TaskRowMetrics::BOX_SIDE / 2.0,
            ),
            plus_glyph.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            hint_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), TaskRowMetrics::leading(0)),
            hint_label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            text_field.leadingAnchor().constraintEqualToAnchor(&hint_label.leadingAnchor()),
            text_field
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&this.trailingAnchor(), -TaskRowMetrics::CONTENT_INSET),
            text_field.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
        ]);
        this
    }

    fn set_on_begin_edit(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_begin_edit.borrow_mut() = handler;
    }

    fn set_on_commit(&self, handler: Option<Rc<dyn Fn(&str)>>) {
        *self.ivars().on_commit.borrow_mut() = handler;
    }

    fn set_on_cancel(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_cancel.borrow_mut() = handler;
    }

    fn configure(&self, editing: bool, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.style_sheet.borrow_mut() = style_sheet;
        let was_editing = ivars.editing.get();
        ivars.editing.set(editing);
        if editing != was_editing || !editing {
            ivars.cancelled.set(false);
            ivars.commit_sent.set(false);
        }
        self.swap_editor_chrome(editing, was_editing != editing);
        if !editing {
            ivars.text_field.setStringValue(&NSString::from_str(""));
        }
        self.setAccessibilityRole(if editing { None } else { Some(role::button()) });
        set_label(self, if editing { "New task title" } else { "Add task" });
        self.apply_style(false);
    }

    /// Hint and field trade places with a crossfade.
    fn swap_editor_chrome(&self, editing: bool, animated: bool) {
        let ivars = self.ivars();
        let text_field: Retained<NSView> = Retained::into_super(Retained::into_super(ivars.text_field.clone()));
        let hint_label: Retained<NSView> = Retained::into_super(Retained::into_super(ivars.hint_label.clone()));
        let (incoming, outgoing) = if editing { (text_field, hint_label) } else { (hint_label, text_field) };
        let reduce_motion = ivars.style_sheet.borrow().reduce_motion;
        if !(animated && self.window().is_some() && !reduce_motion) {
            outgoing.setHidden(true);
            outgoing.setAlphaValue(1.0);
            incoming.setHidden(false);
            incoming.setAlphaValue(1.0);
            return;
        }
        incoming.setAlphaValue(0.0);
        incoming.setHidden(false);
        outgoing.setHidden(false);
        let weak: ObjcWeak<TaskAddRowView> = ObjcWeak::from(self);
        motion::run(
            false,
            motion::QUICK,
            Curve::Decelerate,
            move |_| {
                incoming.animator().setAlphaValue(1.0);
                outgoing.animator().setAlphaValue(0.0);
            },
            Some(Box::new(move || {
                let Some(this) = weak.load() else { return };
                // Re-resolve at completion: a re-configure mid-fade has
                // already swapped the pair the other way.
                let ivars = this.ivars();
                let settled: &NSView = if ivars.editing.get() { &ivars.hint_label } else { &ivars.text_field };
                settled.setHidden(true);
                settled.setAlphaValue(1.0);
            })),
        );
    }

    fn apply_style(&self, animated: bool) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        let warm = self.is_hovered() && !ivars.editing.get();
        ivars.hint_label.setTextColor(Some(if warm { &style_sheet.text_secondary } else { &style_sheet.text_faint }));
        let plus_tint = if warm {
            style_sheet.accent.clone()
        } else {
            style_sheet.text.panel_alpha(if contrast { 0.5 } else { 0.35 }, contrast)
        };
        ivars.plus_glyph.setContentTintColor(Some(&plus_tint));
        ivars.text_field.setTextColor(Some(&style_sheet.text));
        let surface = if warm {
            cg(&style_sheet.text.panel_alpha(if contrast { 0.1 } else { 0.07 }, contrast))
        } else {
            clear_cg_color()
        };
        if !(animated && !style_sheet.reduce_motion && self.window().is_some()) {
            set_surface(self.surface_layer(), &surface);
            return;
        }
        fade_surface(self.surface_layer(), &surface);
    }

    // MARK: - Editing

    fn do_command(&self, command: Sel) -> bool {
        let ivars = self.ivars();
        if command == sel!(cancelOperation:) {
            if !ivars.editing.get() {
                return true;
            }
            ivars.cancelled.set(true);
            ivars.commit_sent.set(true);
            let handler = ivars.on_cancel.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
            return true;
        }
        if command == sel!(insertNewline:) {
            self.commit_if_needed();
            return true;
        }
        false
    }

    fn commit_if_needed(&self) {
        let ivars = self.ivars();
        if !(ivars.editing.get() && !ivars.cancelled.get() && !ivars.commit_sent.get()) {
            return;
        }
        ivars.commit_sent.set(true);
        let handler = ivars.on_commit.borrow().clone();
        let text = ivars.text_field.stringValue().to_string();
        if let Some(handler) = handler {
            handler(&text);
        }
    }

    // MARK: - Pointer

    fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
        if self.ivars().editing.get() {
            return unsafe { msg_send![super(self), hitTest: point] };
        }
        let local_point = superview(self).map_or(point, |superview| self.convertPoint_fromView(point, Some(&superview)));
        if self.bounds().contains_point(local_point) { Some(Retained::into_super(Retained::into_super(self.retain()))) } else { None }
    }

    fn accessibility_perform_press(&self) -> bool {
        if self.ivars().editing.get() {
            return false;
        }
        let handler = self.ivars().on_begin_edit.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
        true
    }
}

// MARK: - Immediate action button

define_class!(
    /// `TaskImmediateActionButton`: the empty state's add button, which acts
    /// on press.
    // SAFETY: no ivars; AppKit's own initialisers are safe to inherit.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskImmediateActionButton"]
    struct TaskImmediateActionButton;

    unsafe impl NSObjectProtocol for TaskImmediateActionButton {}

    impl TaskImmediateActionButton {
        #[unsafe(method(acceptsFirstMouse:))]
        fn __accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            if !self.isEnabled() {
                return;
            }
            self.highlight(true);
            let _ = self.send_action();
            let weak: ObjcWeak<TaskImmediateActionButton> = ObjcWeak::from(self);
            main_async(move || {
                if let Some(this) = weak.load() {
                    this.highlight(false);
                }
            });
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn __accessibility_perform_press(&self) -> bool {
            self.isEnabled() && self.send_action()
        }
    }
);

impl TaskImmediateActionButton {
    /// `TaskImmediateActionButton(title:target:action:)`: NSButton's class
    /// factory, sent to the subclass.
    fn with_title(title: &str, mtm: MainThreadMarker) -> Retained<TaskImmediateActionButton> {
        let _ = mtm;
        unsafe {
            msg_send![
                TaskImmediateActionButton::class(),
                buttonWithTitle: &*ns_string(title),
                target: None::<&AnyObject>,
                action: None::<Sel>
            ]
        }
    }

    /// `sendAction(action, to: target)`.
    fn send_action(&self) -> bool {
        let target = self.target();
        unsafe { self.sendAction_to(self.action(), target.as_deref()) }
    }
}

// MARK: - Undo pill

pub struct TaskUndoPillViewIvars {
    on_undo: Handler,
    on_visibility_change: Handler,
    style_sheet: RefCell<Rc<StyleSheet>>,
    label: Retained<NSTextField>,
    undo_button: Retained<NSButton>,
    undo_action: RefCell<Option<Retained<ButtonAction>>>,
    dismiss_timer: RefCell<Option<Retained<NSTimer>>>,
    tracking_area_ref: RefCell<Option<Retained<NSTrackingArea>>>,
    /// What the pill believes it should be, tracked apart from `isHidden` so
    /// a re-present during a dismiss animation wins over the fade's
    /// completion.
    wants_visible: Cell<bool>,
}

define_class!(
    /// The transient "Completed '…' — Undo" pill.
    // SAFETY: `initWithFrame:` sets the ivars (Swift overrides
    // `init(frame:)`).
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskUndoPillView"]
    #[ivars = TaskUndoPillViewIvars]
    struct TaskUndoPillView;

    unsafe impl NSObjectProtocol for TaskUndoPillView {}

    impl TaskUndoPillView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("TaskUndoPillView is created on the main thread");
            let this = this.set_ivars(TaskUndoPillViewIvars {
                on_undo: RefCell::new(None),
                on_visibility_change: RefCell::new(None),
                style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
                label: label("", mtm),
                undo_button: NSButton::new(mtm),
                undo_action: RefCell::new(None),
                dismiss_timer: RefCell::new(None),
                tracking_area_ref: RefCell::new(None),
                wants_visible: Cell::new(false),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.finish_init(mtm);
            this
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            let label_size = self.ivars().label.intrinsicContentSize();
            let button_size = self.ivars().undo_button.intrinsicContentSize();
            NSSize::new(12.0 + smin(label_size.width, 210.0) + 8.0 + button_size.width + 10.0, Self::HEIGHT)
        }

        /// The pointer pauses the countdown.
        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area_ref,
                NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveInKeyWindow,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            let timer = self.ivars().dismiss_timer.borrow_mut().take();
            if let Some(timer) = timer {
                timer.invalidate();
            }
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.arm_dismiss(1.6);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }
);

impl TaskUndoPillView {
    pub const HEIGHT: CGFloat = 28.0;

    /// `TaskUndoPillView()`.
    fn new(mtm: MainThreadMarker) -> Retained<TaskUndoPillView> {
        unsafe { msg_send![TaskUndoPillView::alloc(mtm), init] }
    }

    fn finish_init(&self, mtm: MainThreadMarker) {
        self.setWantsLayer(true);
        if let Some(layer) = self.layer() {
            layer.setCornerRadius(PanelMetrics::capsule_radius(Self::HEIGHT));
            layer.setShadowOpacity(0.18);
            layer.setShadowRadius(5.0);
            layer.setShadowOffset(NSSize::new(0.0, -1.0));
        }

        let ivars = self.ivars();
        let label = &ivars.label;
        label.setFont(Some(&PanelFont::secondary()));
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        self.addSubview(label);

        let weak: ObjcWeak<TaskUndoPillView> = ObjcWeak::from(self);
        let action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    let handler = this.ivars().on_undo.borrow().clone();
                    if let Some(handler) = handler {
                        handler();
                    }
                }
                if let Some(this) = weak.load() {
                    this.dismiss(true);
                }
            },
            mtm,
        );
        *ivars.undo_action.borrow_mut() = Some(action.clone());
        let undo_button = &ivars.undo_button;
        unsafe {
            undo_button.setTarget(Some(object(&*action)));
            undo_button.setAction(Some(ButtonAction::selector()));
        }
        undo_button.setTitle(&ns_string("Undo"));
        undo_button.setBordered(false);
        undo_button.setFont(Some(&PanelFont::system(11.5, weight_semibold())));
        set_role(&**undo_button, role::button());
        set_label(&**undo_button, "Undo");
        undo_button.setToolTip(Some(&ns_string("Undo completing this task")));
        undo_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        undo_button.setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
        self.addSubview(undo_button);

        activate(&[
            label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 12.0),
            label.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            undo_button.leadingAnchor().constraintEqualToAnchor_constant(&label.trailingAnchor(), 8.0),
            undo_button.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -10.0),
            undo_button.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
        ]);
        self.apply_style();
    }

    fn set_on_undo(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_undo.borrow_mut() = handler;
    }

    fn set_on_visibility_change(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_visibility_change.borrow_mut() = handler;
    }

    /// `styleSheet` with its `didSet`.
    fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    fn present(&self, title: &str) {
        let ivars = self.ivars();
        let trimmed = swift_text::trim_whitespaces(title);
        ivars.label.setStringValue(&ns_string(&format!("Completed ‘{trimmed}’")));
        self.invalidateIntrinsicContentSize();
        self.arm_dismiss(4.0);
        ivars.wants_visible.set(true);
        if !self.isHidden() {
            // A second completion while the pill is out retells it and
            // cancels any fade already in flight.
            if let Some(layer) = self.layer() {
                layer.removeAnimationForKey(&NSString::from_str("transform"));
            }
            self.setAlphaValue(1.0);
            return;
        }
        self.set_hidden_notifying(false);
        let reduce_motion = ivars.style_sheet.borrow().reduce_motion;
        if !(!reduce_motion && self.window().is_some()) {
            self.setAlphaValue(1.0);
            return;
        }
        self.setAlphaValue(0.0);
        if let Some(layer) = self.layer() {
            layer.setTransform(CATransform3D::new_translation(0.0, 6.0, 0.0));
        }
        let this = self.retain();
        motion::run(
            false,
            motion::DELIBERATE,
            Curve::Structural,
            move |_| {
                this.animator().setAlphaValue(1.0);
                if let Some(layer) = this.layer() {
                    layer.setTransform(unsafe { CATransform3DIdentity });
                }
            },
            None,
        );
    }

    fn dismiss(&self, animated: bool) {
        let ivars = self.ivars();
        let timer = ivars.dismiss_timer.borrow_mut().take();
        if let Some(timer) = timer {
            timer.invalidate();
        }
        ivars.wants_visible.set(false);
        let reduce_motion = ivars.style_sheet.borrow().reduce_motion;
        if !(animated && !reduce_motion && self.window().is_some()) {
            self.set_hidden_notifying(true);
            return;
        }
        let this = self.retain();
        let finished = self.retain();
        motion::run(
            false,
            motion::STANDARD,
            Curve::EaseOut,
            move |_| this.animator().setAlphaValue(0.0),
            Some(Box::new(move || {
                // A re-present during the fade outranks the fade.
                if finished.ivars().wants_visible.get() {
                    return;
                }
                finished.set_hidden_notifying(true);
                if let Some(layer) = finished.layer() {
                    layer.setTransform(unsafe { CATransform3DIdentity });
                }
            })),
        );
    }

    /// Swift's private `setHidden(_:)` (not an Objective-C override).
    fn set_hidden_notifying(&self, hidden: bool) {
        if self.isHidden() == hidden {
            return;
        }
        self.setHidden(hidden);
        let handler = self.ivars().on_visibility_change.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn arm_dismiss(&self, interval: f64) {
        let previous = self.ivars().dismiss_timer.borrow_mut().take();
        if let Some(previous) = previous {
            previous.invalidate();
        }
        let weak: ObjcWeak<TaskUndoPillView> = ObjcWeak::from(self);
        let block = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
            if let Some(this) = weak.load() {
                this.dismiss(true);
            }
        });
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(interval, false, &block) };
        *self.ivars().dismiss_timer.borrow_mut() = Some(timer);
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.text.colorWithAlphaComponent(if contrast { 0.22 } else { 0.14 }))));
        }
        if let Some(layer) = self.layer() {
            layer.setBorderWidth(if contrast { 1.0 } else { 0.5 });
        }
        if let Some(layer) = self.layer() {
            layer.setBorderColor(Some(&cg(&style_sheet.text.panel_alpha(if contrast { 0.45 } else { 0.22 }, false))));
        }
        ivars.label.setTextColor(Some(&style_sheet.text));
        ivars.undo_button.setContentTintColor(Some(&style_sheet.accent));
    }
}
