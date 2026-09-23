//! Port of `Panels/ReviewPanelView.swift`: the review list (comments and
//! suggestions left beside the document), with Apply / Reject / Resolve.
//!
//! The panel is its table's data source and delegate, as in Swift; the
//! private `ReviewRowView` is a `define_class!` `NSTableCellView` subclass
//! with the same Objective-C name.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSButton, NSControlTextEditingDelegate, NSLayoutAttribute, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow,
    NSLineBreakMode, NSResponder, NSScrollView, NSStackView, NSTableCellView, NSTableColumn, NSTableRowView,
    NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextAlignment, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSNotification, NSRect, NSString};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text::ns::foundation::{capitalized, replacing_occurrences};

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role, set_tool_tip, set_value};
use super::panel_chrome::{
    ButtonAction, PanelBackdrop, PanelButton, PanelEmptyStateView, PanelFont, PanelList, PanelMetrics, PanelSurface,
    PanelTableView, install_backdrop, panel_title,
};
use crate::review::review_anchor_resolver::{CONTEXT_LENGTH, ReviewAnchorResolver};
use crate::review::review_sidecar::{ReviewAnchor, ReviewAnchorStatus, ReviewItem, ReviewKind, ReviewState};
use crate::support::commands::Command;

/// `ReviewPanelViewDelegate`.
pub trait ReviewPanelViewDelegate {
    fn review_panel_did_select(&self, panel: &ReviewPanelView, review: &ReviewItem);
    fn review_panel_did_apply(&self, panel: &ReviewPanelView, review: &ReviewItem);
    fn review_panel_did_reject(&self, panel: &ReviewPanelView, review: &ReviewItem);
    fn review_panel_did_resolve(&self, panel: &ReviewPanelView, review: &ReviewItem);
}

pub struct ReviewPanelViewIvars {
    delegate: RefCell<Option<Weak<dyn ReviewPanelViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    reviews: RefCell<Vec<ReviewItem>>,
    source_text: RefCell<String>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
    empty_state: Retained<PanelEmptyStateView>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`, created when `buildTable` first reads it.
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    button_row: Retained<NSStackView>,
    apply_button: RefCell<Option<Retained<NSButton>>>,
    reject_button: RefCell<Option<Retained<NSButton>>>,
    resolve_button: RefCell<Option<Retained<NSButton>>>,
    actions: RefCell<Vec<Retained<ButtonAction>>>,
    /// Anchor resolution memoised per document revision.  Resolving every
    /// anchor over the whole document per row per reload made the panel
    /// cost O(reviews × document) on the main thread for every keystroke.
    resolutions: RefCell<HashMap<ReviewAnchor, ReviewAnchorStatus>>,
    /// What the last action did.  Rows disappearing is not feedback.
    action_status: RefCell<Option<String>>,
    is_acting: Cell<bool>,
}

define_class!(
    /// `ReviewPanelView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ReviewPanelView"]
    #[ivars = ReviewPanelViewIvars]
    pub struct ReviewPanelView;

    unsafe impl NSObjectProtocol for ReviewPanelView {}

    impl ReviewPanelView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        #[unsafe(method(rowClicked:))]
        fn __row_clicked(&self, _sender: &NSTableView) {
            self.activate_selection();
        }
    }

    unsafe impl NSControlTextEditingDelegate for ReviewPanelView {}

    unsafe impl NSTableViewDataSource for ReviewPanelView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().reviews.borrow().len() as isize
        }
    }

    unsafe impl NSTableViewDelegate for ReviewPanelView {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn __row_view_for_row(&self, table_view: &NSTableView, _row: isize) -> Option<Retained<NSTableRowView>> {
            let row = PanelList::selection_row(table_view, Some(object(self)), self.style_sheet(), self.mtm());
            Some(Retained::into_super(row))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn __selection_did_change(&self, _notification: &NSNotification) {
            self.update_action_state();
            if let Some(review) = self.selected_review()
                && let Some(delegate) = self.delegate()
            {
                delegate.review_panel_did_select(self, &review);
            }
        }
    }
);

impl PanelSurface for ReviewPanelView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl ReviewPanelView {
    /// `ReviewPanelView()`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<ReviewPanelView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<ReviewPanelView> {
        let title_label = label(&panel_title(Command::ReviewPanel), mtm);
        let status_label = label("", mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let table = PanelList::make_table_view("reviews", mtm);
        let button_row = NSStackView::new(mtm);
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(ReviewPanelViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            reviews: RefCell::new(Vec::new()),
            source_text: RefCell::new(String::new()),
            backdrop: backdrop.clone(),
            title_label,
            status_label,
            empty_state,
            table,
            scroll: RefCell::new(None),
            button_row,
            apply_button: RefCell::new(None),
            reject_button: RefCell::new(None),
            resolve_button: RefCell::new(None),
            actions: RefCell::new(Vec::new()),
            resolutions: RefCell::new(HashMap::new()),
            action_status: RefCell::new(None),
            is_acting: Cell::new(false),
        });
        let this: Retained<ReviewPanelView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        install_backdrop(&this, &backdrop);
        this.build_header(mtm);
        this.build_table(mtm);
        this.apply_style();
        this.reload();
        set_role(&*this, role::group());
        set_label(&*this, "Reviews");
        this
    }

    /// `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self, mtm: MainThreadMarker) -> Retained<NSScrollView> {
        if let Some(scroll) = self.ivars().scroll.borrow().as_ref() {
            return scroll.clone();
        }
        let scroll = PanelList::make_scroll_view(&self.ivars().table, mtm);
        *self.ivars().scroll.borrow_mut() = Some(scroll.clone());
        scroll
    }

    fn build_header(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        let status_label = &ivars.status_label;
        title_label.setFont(Some(&PanelFont::header()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);

        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setAlignment(NSTextAlignment::Right);
        status_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(status_label);

        let weak: ObjcWeak<ReviewPanelView> = ObjcWeak::from(self);
        let apply_action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.apply_selected();
                }
            },
            mtm,
        );
        let weak: ObjcWeak<ReviewPanelView> = ObjcWeak::from(self);
        let resolve_action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.resolve_selected();
                }
            },
            mtm,
        );
        let weak: ObjcWeak<ReviewPanelView> = ObjcWeak::from(self);
        let reject_action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.reject_selected();
                }
            },
            mtm,
        );
        let apply = PanelButton::text("Apply", &apply_action, false, mtm);
        let reject = PanelButton::text("Reject", &reject_action, false, mtm);
        let resolve = PanelButton::text("Resolve", &resolve_action, false, mtm);
        *ivars.actions.borrow_mut() = vec![apply_action, reject_action, resolve_action];
        for button in [&apply, &reject, &resolve] {
            button.setTranslatesAutoresizingMaskIntoConstraints(false);
            button.setContentCompressionResistancePriority_forOrientation(
                NSLayoutPriorityDefaultLow,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }
        let button_row = &ivars.button_row;
        button_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        button_row.setAlignment(NSLayoutAttribute::CenterY);
        button_row.setSpacing(6.0);
        button_row.setTranslatesAutoresizingMaskIntoConstraints(false);
        button_row.addArrangedSubview(&apply);
        button_row.addArrangedSubview(&reject);
        button_row.addArrangedSubview(&resolve);
        self.addSubview(button_row);
        *ivars.apply_button.borrow_mut() = Some(apply);
        *ivars.reject_button.borrow_mut() = Some(reject);
        *ivars.resolve_button.borrow_mut() = Some(resolve);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 8.0),
            status_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            status_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            status_label
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            button_row.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            button_row
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            button_row.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 7.0),
        ]);
    }

    fn build_table(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let table = &ivars.table;
        // SAFETY: the table is the panel's own subview and never outlives it
        // (the data source and delegate are weak in AppKit, as in Swift).
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        table.setRowHeight(PanelMetrics::WIDE_ROW_HEIGHT);
        unsafe {
            table.setTarget(Some(object(self)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<ReviewPanelView> = ObjcWeak::from(self);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&**table, "Review items");
        let scroll = self.scroll(mtm);
        self.addSubview(&scroll);
        ivars.empty_state.install(self, &scroll, 1.0);
        // Anchored under the buttons rather than 70pt below the title: a
        // magic constant stops being right the moment a button grows.
        activate(&[
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&ivars.button_row.bottomAnchor(), 8.0),
            scroll.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);
    }

    pub fn delegate(&self) -> Option<Rc<dyn ReviewPanelViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn ReviewPanelViewDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn reviews(&self) -> Vec<ReviewItem> {
        self.ivars().reviews.borrow().clone()
    }

    pub fn set_reviews(&self, reviews: Vec<ReviewItem>) {
        *self.ivars().reviews.borrow_mut() = reviews;
        self.reload();
    }

    pub fn source_text(&self) -> String {
        self.ivars().source_text.borrow().clone()
    }

    pub fn set_source_text(&self, text: &str) {
        let changed = !upleft_swift_text::str_eq(text, &self.ivars().source_text.borrow());
        *self.ivars().source_text.borrow_mut() = text.to_owned();
        if !changed {
            return;
        }
        // The document changed, so every anchor has to be resolved again —
        // but only once each, not once per row per reload.
        self.ivars().resolutions.borrow_mut().clear();
        self.reload();
    }

    pub fn reload(&self) {
        let ivars = self.ivars();
        if !ivars.is_acting.get() {
            *ivars.action_status.borrow_mut() = None;
        }
        ivars.table.reloadData();
        let open = ivars.reviews.borrow().iter().filter(|review| review.state == ReviewState::Open).count();
        let action_status = ivars.action_status.borrow().clone();
        let status = match action_status {
            Some(status) => status,
            None => {
                if open == 0 {
                    "No open reviews".to_owned()
                } else {
                    format!("{open} open")
                }
            }
        };
        ivars.status_label.setStringValue(&ns_string(&status));
        set_label(&*ivars.status_label, &ivars.status_label.stringValue().to_string());
        self.update_empty_state();
        self.update_action_state();
    }

    fn update_empty_state(&self) {
        let ivars = self.ivars();
        let scroll = ivars.scroll.borrow().clone();
        if !ivars.reviews.borrow().is_empty() {
            ivars.empty_state.setHidden(true);
            if let Some(scroll) = &scroll {
                scroll.setHidden(false);
            }
            return;
        }
        ivars.empty_state.configure(
            "bubble.left.and.bubble.right",
            "No reviews",
            "Comments and suggestions left beside\nthis document will appear here.",
            &self.style_sheet(),
        );
        ivars.empty_state.setHidden(false);
        if let Some(scroll) = &scroll {
            scroll.setHidden(true);
        }
    }

    fn update_action_state(&self) {
        let ivars = self.ivars();
        let review = self.selected_review();
        let is_suggestion = review.as_ref().map(|review| review.kind) == Some(ReviewKind::Suggestion);
        if let Some(button) = ivars.apply_button.borrow().as_ref() {
            button.setEnabled(is_suggestion);
        }
        if let Some(button) = ivars.reject_button.borrow().as_ref() {
            button.setEnabled(is_suggestion);
        }
        if let Some(button) = ivars.resolve_button.borrow().as_ref() {
            button.setEnabled(review.is_some());
        }
    }

    fn selected_review(&self) -> Option<ReviewItem> {
        let row = self.ivars().table.selectedRow();
        let reviews = self.ivars().reviews.borrow();
        if !(row >= 0 && (row as usize) < reviews.len()) {
            return None;
        }
        Some(reviews[row as usize].clone())
    }

    fn activate_selection(&self) {
        let Some(review) = self.selected_review() else { return };
        if let Some(delegate) = self.delegate() {
            delegate.review_panel_did_select(self, &review);
        }
    }

    fn apply_selected(&self) {
        let Some(review) = self.selected_review() else { return };
        if review.kind != ReviewKind::Suggestion {
            return;
        }
        self.act(&format!("Applied “{}”.", review.title()), || {
            if let Some(delegate) = self.delegate() {
                delegate.review_panel_did_apply(self, &review);
            }
        });
    }

    fn resolve_selected(&self) {
        let Some(review) = self.selected_review() else { return };
        self.act(&format!("Resolved “{}”.", review.title()), || {
            if let Some(delegate) = self.delegate() {
                delegate.review_panel_did_resolve(self, &review);
            }
        });
    }

    fn reject_selected(&self) {
        let Some(review) = self.selected_review() else { return };
        if review.kind != ReviewKind::Suggestion {
            return;
        }
        self.act(&format!("Rejected “{}”.", review.title()), || {
            if let Some(delegate) = self.delegate() {
                delegate.review_panel_did_reject(self, &review);
            }
        });
    }

    /// Reports the outcome in the status label and keeps it there through
    /// the reload the action itself causes.
    fn act(&self, message: &str, body: impl FnOnce()) {
        let ivars = self.ivars();
        *ivars.action_status.borrow_mut() = Some(message.to_owned());
        ivars.is_acting.set(true);
        body();
        ivars.status_label.setStringValue(&ns_string(message));
        set_label(&*ivars.status_label, message);
        ivars.is_acting.set(false);
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.status_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.table.reloadData();
        self.update_empty_state();
    }

    // MARK: - Table

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let review = {
            let reviews = self.ivars().reviews.borrow();
            if !(row >= 0 && (row as usize) < reviews.len()) {
                return None;
            }
            reviews[row as usize].clone()
        };
        let identifier = NSString::from_str("reviewRow");
        let reused = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
            .and_then(|view| view.downcast::<ReviewRowView>().ok());
        let cell = reused.unwrap_or_else(|| ReviewRowView::new(&identifier, self.mtm()));
        let status = self.status(&review.anchor);
        cell.configure(&review, status, &self.style_sheet());
        Some(Retained::into_super(Retained::into_super(cell)))
    }

    /// One resolution per anchor per document revision.
    fn status(&self, anchor: &ReviewAnchor) -> ReviewAnchorStatus {
        if let Some(cached) = self.ivars().resolutions.borrow().get(anchor) {
            return *cached;
        }
        let status = ReviewAnchorResolver::resolve(anchor, &self.ivars().source_text.borrow(), CONTEXT_LENGTH).status;
        self.ivars().resolutions.borrow_mut().insert(anchor.clone(), status);
        status
    }

    /// The Apply, Reject and Resolve buttons, in that order.
    pub fn buttons_for_testing(&self) -> Vec<Retained<NSButton>> {
        let ivars = self.ivars();
        [&ivars.apply_button, &ivars.reject_button, &ivars.resolve_button]
            .iter()
            .filter_map(|button| button.borrow().clone())
            .collect()
    }

    pub fn table_for_testing(&self) -> Retained<PanelTableView> {
        self.ivars().table.clone()
    }

    pub fn status_text_for_testing(&self) -> String {
        self.ivars().status_label.stringValue().to_string()
    }
}

// MARK: - ReviewRowView

pub struct ReviewRowViewIvars {
    title_label: Retained<NSTextField>,
    body_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
}

define_class!(
    /// `ReviewRowView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSTableCellView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ReviewRowView"]
    #[ivars = ReviewRowViewIvars]
    pub struct ReviewRowView;

    unsafe impl NSObjectProtocol for ReviewRowView {}
);

impl ReviewRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<ReviewRowView> {
        let title_label = label("", mtm);
        let body_label = label("", mtm);
        let status_label = label("", mtm);
        let this = Self::alloc(mtm).set_ivars(ReviewRowViewIvars {
            title_label: title_label.clone(),
            body_label: body_label.clone(),
            status_label: status_label.clone(),
        });
        let this: Retained<ReviewRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        title_label.setFont(Some(&PanelFont::row_emphasised()));
        body_label.setFont(Some(&PanelFont::secondary()));
        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setAlignment(NSTextAlignment::Right);
        for field in [&title_label, &body_label, &status_label] {
            field.setTranslatesAutoresizingMaskIntoConstraints(false);
            this.addSubview(field);
        }
        body_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 12.0),
            title_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 7.0),
            title_label
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&status_label.leadingAnchor(), -6.0),
            status_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -12.0),
            status_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            body_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            body_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -12.0),
            body_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 4.0),
            body_label.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&this.bottomAnchor(), -7.0),
        ]);
        set_role(&*this, role::static_text());
        this
    }

    fn configure(&self, review: &ReviewItem, status: ReviewAnchorStatus, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        ivars.title_label.setStringValue(&ns_string(review.title()));
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        let body = if review.kind == ReviewKind::Suggestion {
            format!(
                "Replace {} with {}",
                Self::quoted(&review.anchor.selected_text),
                Self::quoted(review.replacement.as_deref().unwrap_or(""))
            )
        } else {
            review.body.clone()
        };
        ivars.body_label.setStringValue(&ns_string(&body));
        ivars.body_label.setTextColor(Some(&style_sheet.text_secondary));
        let state = if review.state == ReviewState::Open {
            capitalized(status.raw_value())
        } else {
            capitalized(review.state.raw_value())
        };
        ivars.status_label.setStringValue(&ns_string(&state));
        ivars.status_label.setTextColor(Some(&style_sheet.text_faint));
        set_tool_tip(self, Some(&review.body));
        set_label(self, &format!("{}: {}", review.title(), review.body));
        set_value(self, &state);
    }

    fn quoted(value: &str) -> String {
        let compact = replacing_occurrences(value, "\n", " ↵ ");
        format!("“{compact}”")
    }
}

#[allow(unused)]
fn _unused(_: &AnyObject, _: &dyn NSAccessibility) {}
