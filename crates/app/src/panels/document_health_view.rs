//! Port of `Panels/DocumentHealthView.swift`: a source-first health report.
//! The panel owns only presentation state. The document controller owns
//! parsing, selection, and edits.
//!
//! Objective-C class names: `DocumentHealthView`, `HealthDiagnosticRowView`.
//! The table data source and delegate are the view itself, as in Swift, so
//! `numberOfRowsInTableView:`, `tableView:viewForTableColumn:row:` and the
//! other callbacks are Objective-C methods on it (the tests call them).

// `!(a < b)` spells Swift's `guard a < b`; the negated comparisons are
// deliberate.
#![allow(clippy::nonminimal_bool)]

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlTextEditingDelegate, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLineBreakMode, NSResponder, NSScrollView,
    NSStackView, NSTableColumn, NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate,
    NSTextAlignment, NSTextField, NSTextView, NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation,
    NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSIndexSet, NSRect, NSSize, NSString};
use upleft_core::contracts::TextEdit;
use upleft_core::health::document_health::{DocumentHealthCategory, DocumentHealthDiagnostic, DocumentHealthSeverity};
use upleft_render::core_types::CalloutKind;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role};
use super::panel_chrome::{
    ButtonAction, PanelBackdrop, PanelButton, PanelEmptyStateView, PanelFont, PanelGroupRowView, PanelList,
    PanelMetrics, PanelSurface, PanelTableView, SourceLineIndex, install_backdrop, panel_title,
};
use crate::support::commands::Command;

/// `DocumentHealthViewDelegate`.
pub trait DocumentHealthViewDelegate {
    fn document_health_view_did_select(&self, view: &DocumentHealthView, diagnostic: &DocumentHealthDiagnostic);
    fn document_health_view_did_apply(&self, view: &DocumentHealthView, fixes: &[TextEdit]);
    fn document_health_view_wants_source_mode(&self, view: &DocumentHealthView);
}

/// `DocumentHealthView.Row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Group(DocumentHealthCategory, DocumentHealthSeverity, usize),
    Diagnostic(usize),
}

pub struct DocumentHealthViewIvars {
    delegate: RefCell<Option<Weak<dyn DocumentHealthViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    source_text: RefCell<String>,
    diagnostics: RefCell<Vec<DocumentHealthDiagnostic>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    count_label: Retained<NSTextField>,
    preview_label: Retained<NSTextField>,
    preview: Retained<NSTextView>,
    empty_state: Retained<PanelEmptyStateView>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`.
    scroll: OnceCell<Retained<NSScrollView>>,
    apply_button: Retained<NSButton>,
    ignore_button: Retained<NSButton>,
    source_button: Retained<NSButton>,
    apply_action: RefCell<Option<Retained<ButtonAction>>>,
    ignore_action: RefCell<Option<Retained<ButtonAction>>>,
    source_action: RefCell<Option<Retained<ButtonAction>>>,
    ignored_ids: RefCell<HashSet<String>>,
    rows: RefCell<Vec<Row>>,
    selected_index: Cell<Option<usize>>,
    /// Selection is remembered by finding id, not row: the host reparses on
    /// every keystroke and a row number does not survive that.
    selected_diagnostic_id: RefCell<Option<String>>,
    line_index: RefCell<Rc<SourceLineIndex>>,
    /// What the last action did, shown where the preview normally is.
    /// Cleared by the first reload that is not the one the action itself
    /// caused.
    action_status: RefCell<Option<String>>,
    is_applying_fixes: Cell<bool>,
    preview_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
}

define_class!(
    /// `DocumentHealthView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentHealthView"]
    #[ivars = DocumentHealthViewIvars]
    pub struct DocumentHealthView;

    unsafe impl NSObjectProtocol for DocumentHealthView {}

    impl DocumentHealthView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::DETAIL_WIDTH
        }

        #[unsafe(method(rowClicked:))]
        fn __row_clicked(&self, _sender: Option<&AnyObject>) {
            self.activate_selection();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }

    unsafe impl NSTableViewDataSource for DocumentHealthView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for DocumentHealthView {}

    unsafe impl NSTableViewDelegate for DocumentHealthView {
        #[unsafe(method(tableView:heightOfRow:))]
        fn __height_of_row(&self, _table_view: &NSTableView, row: isize) -> CGFloat {
            self.height_of_row(row)
        }

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn __row_view_for_row(&self, table_view: &NSTableView, _row: isize) -> Option<Retained<NSTableRowView>> {
            let row_view = PanelList::selection_row(table_view, Some(object(self)), self.style_sheet(), self.mtm());
            Some(Retained::into_super(row_view))
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn __is_group_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.is_group_row(row)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn __should_select_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.should_select_row(row)
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }
    }
);

impl PanelSurface for DocumentHealthView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl DocumentHealthView {
    /// `DocumentHealthView()`: `init(styleSheet: .current)`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<DocumentHealthView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<DocumentHealthView> {
        // Stored property initial values, in declaration order.
        let title_label = label(&panel_title(Command::DocumentHealth), mtm);
        let count_label = label("", mtm);
        let preview_label = label("", mtm);
        let preview = NSTextView::new(mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let table = PanelList::make_table_view("documentHealth", mtm);
        // The initialiser body before `super.init`.
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let apply_button = PanelButton::text("Apply safe fixes", &ButtonAction::noop(mtm), false, mtm);
        let ignore_button = PanelButton::text("Ignore", &ButtonAction::noop(mtm), false, mtm);
        let source_button = PanelButton::text("Open Source Focus", &ButtonAction::noop(mtm), false, mtm);
        let this = Self::alloc(mtm).set_ivars(DocumentHealthViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            source_text: RefCell::new(String::new()),
            diagnostics: RefCell::new(Vec::new()),
            backdrop,
            title_label,
            count_label,
            preview_label,
            preview,
            empty_state,
            table,
            scroll: OnceCell::new(),
            apply_button,
            ignore_button,
            source_button,
            apply_action: RefCell::new(None),
            ignore_action: RefCell::new(None),
            source_action: RefCell::new(None),
            ignored_ids: RefCell::new(HashSet::new()),
            rows: RefCell::new(Vec::new()),
            selected_index: Cell::new(None),
            selected_diagnostic_id: RefCell::new(None),
            line_index: RefCell::new(Rc::new(SourceLineIndex::new(""))),
            action_status: RefCell::new(None),
            is_applying_fixes: Cell::new(false),
            preview_constraints: RefCell::new(Vec::new()),
        });
        let this: Retained<DocumentHealthView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.finish_init(mtm);
        this
    }

    fn finish_init(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        install_backdrop(self, &ivars.backdrop);

        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::header()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);
        let count_label = &ivars.count_label;
        count_label.setFont(Some(&PanelFont::secondary()));
        count_label.setAlignment(NSTextAlignment::Right);
        count_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(count_label);

        let table = &ivars.table;
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
            table.setTarget(Some(object(self)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<DocumentHealthView> = ObjcWeak::from(self);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&**table, "Document health findings");
        let scroll = self.scroll();
        self.addSubview(&scroll);

        let preview_label = &ivars.preview_label;
        preview_label.setFont(Some(&PanelFont::secondary()));
        preview_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(preview_label);
        let preview = &ivars.preview;
        preview.setEditable(false);
        preview.setSelectable(true);
        preview.setRichText(false);
        preview.setFont(Some(&PanelFont::monospaced_regular(10.0)));
        preview.setDrawsBackground(true);
        preview.setTextContainerInset(NSSize::new(6.0, 5.0));
        set_label(&**preview, "Source change preview");
        preview.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(preview);

        let weak: ObjcWeak<DocumentHealthView> = ObjcWeak::from(self);
        let apply = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.apply_safe_fixes();
                }
            },
            mtm,
        );
        *ivars.apply_action.borrow_mut() = Some(apply.clone());
        unsafe {
            ivars.apply_button.setTarget(Some(&apply));
            ivars.apply_button.setAction(Some(ButtonAction::selector()));
        }
        set_label(&*ivars.apply_button, "Apply all safe health fixes");

        let weak: ObjcWeak<DocumentHealthView> = ObjcWeak::from(self);
        let ignore = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.ignore_selection();
                }
            },
            mtm,
        );
        *ivars.ignore_action.borrow_mut() = Some(ignore.clone());
        unsafe {
            ivars.ignore_button.setTarget(Some(&ignore));
            ivars.ignore_button.setAction(Some(ButtonAction::selector()));
        }
        set_label(&*ivars.ignore_button, "Ignore selected health finding");

        let weak: ObjcWeak<DocumentHealthView> = ObjcWeak::from(self);
        let source = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.document_health_view_wants_source_mode(&this);
                }
            },
            mtm,
        );
        *ivars.source_action.borrow_mut() = Some(source.clone());
        unsafe {
            ivars.source_button.setTarget(Some(&source));
            ivars.source_button.setAction(Some(ButtonAction::selector()));
        }
        set_label(&*ivars.source_button, "Open Source Focus");

        // Two short rows rather than one 305pt row: the inspector can be
        // narrower than three buttons side by side, and a control that runs
        // off the panel is worse than one that wraps.
        let primary_row = stack_view(&[&ivars.apply_button, &ivars.ignore_button], mtm);
        primary_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        primary_row.setAlignment(NSLayoutAttribute::CenterY);
        primary_row.setSpacing(5.0);
        for button in [&ivars.apply_button, &ivars.ignore_button, &ivars.source_button] {
            button.setContentCompressionResistancePriority_forOrientation(
                NSLayoutPriorityDefaultLow,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }

        let actions = stack_view(&[&primary_row, &ivars.source_button], mtm);
        actions.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        actions.setAlignment(NSLayoutAttribute::Leading);
        actions.setSpacing(5.0);
        actions.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&actions);
        ivars.empty_state.install(self, &scroll, 1.0);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            count_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            count_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            count_label
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 6.0),
            scroll.heightAnchor().constraintGreaterThanOrEqualToConstant(160.0),
            preview_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            preview_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            preview_label.topAnchor().constraintEqualToAnchor_constant(&scroll.bottomAnchor(), 8.0),
            preview.leadingAnchor().constraintEqualToAnchor(&preview_label.leadingAnchor()),
            preview.trailingAnchor().constraintEqualToAnchor(&preview_label.trailingAnchor()),
            preview.topAnchor().constraintEqualToAnchor_constant(&preview_label.bottomAnchor(), 4.0),
            actions.leadingAnchor().constraintEqualToAnchor(&preview_label.leadingAnchor()),
            actions.trailingAnchor().constraintLessThanOrEqualToAnchor(&preview_label.trailingAnchor()),
            actions.topAnchor().constraintEqualToAnchor_constant(&preview.bottomAnchor(), 7.0),
            actions.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&self.bottomAnchor(), -8.0),
        ]);

        // The diff box only exists while there is a change to show in it.
        let preview_constraints = vec![preview.heightAnchor().constraintEqualToConstant(74.0)];
        *ivars.preview_constraints.borrow_mut() = preview_constraints.clone();
        activate(&preview_constraints);

        set_role(self, role::group());
        set_label(self, "Document health");
        self.apply_style();
        self.reload();
    }

    /// Swift's `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self) -> Retained<NSScrollView> {
        let ivars = self.ivars();
        ivars.scroll.get_or_init(|| PanelList::make_scroll_view(&ivars.table, self.mtm())).clone()
    }

    // MARK: Properties

    pub fn delegate(&self) -> Option<Rc<dyn DocumentHealthViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn DocumentHealthViewDelegate>>) {
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

    pub fn source_text(&self) -> String {
        self.ivars().source_text.borrow().clone()
    }

    pub fn set_source_text(&self, source_text: &str) {
        let ivars = self.ivars();
        // Swift's `!=` on `String` is canonical equivalence.
        let unchanged = swift::str_eq(source_text, &ivars.source_text.borrow());
        *ivars.source_text.borrow_mut() = source_text.to_owned();
        if unchanged {
            return;
        }
        *ivars.line_index.borrow_mut() = Rc::new(SourceLineIndex::new(source_text));
        ivars.table.reloadData();
        self.reload_preview();
    }

    pub fn diagnostics(&self) -> Vec<DocumentHealthDiagnostic> {
        self.ivars().diagnostics.borrow().clone()
    }

    pub fn set_diagnostics(&self, diagnostics: Vec<DocumentHealthDiagnostic>) {
        *self.ivars().diagnostics.borrow_mut() = diagnostics;
        self.reload();
    }

    pub fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }

    // MARK: Actions

    pub fn reset_ignored_findings(&self) {
        self.ivars().ignored_ids.borrow_mut().clear();
        self.reload();
    }

    pub fn select_finding_for_testing(&self, row: isize) {
        let ivars = self.ivars();
        {
            let rows = ivars.rows.borrow();
            if !(row >= 0 && (row as usize) < rows.len() && matches!(rows[row as usize], Row::Diagnostic(_))) {
                return;
            }
        }
        ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        ivars.selected_index.set(self.diagnostic_index(row));
        self.reload_preview();
    }

    pub fn apply_safe_fixes_for_testing(&self) {
        self.apply_safe_fixes();
    }

    pub fn ignore_selection_for_testing(&self) {
        self.ignore_selection();
    }

    fn visible_diagnostics(&self) -> Vec<usize> {
        let ivars = self.ivars();
        let diagnostics = ivars.diagnostics.borrow();
        let ignored = ivars.ignored_ids.borrow();
        (0..diagnostics.len()).filter(|index| !ignored.contains(&diagnostics[*index].id)).collect()
    }

    fn reload(&self) {
        let ivars = self.ivars();
        if !ivars.is_applying_fixes.get() {
            *ivars.action_status.borrow_mut() = None;
        }
        let visible = self.visible_diagnostics();
        {
            let diagnostics = ivars.diagnostics.borrow();
            let mut rows = ivars.rows.borrow_mut();
            rows.clear();
            // `Dictionary(grouping:)`: each group keeps the indices' order.
            let mut grouped: HashMap<DocumentHealthCategory, Vec<usize>> = HashMap::new();
            for index in &visible {
                grouped.entry(diagnostics[*index].category).or_default().push(*index);
            }
            for category in DocumentHealthCategory::ALL_CASES {
                let Some(indices) = grouped.get(&category) else { continue };
                if indices.is_empty() {
                    continue;
                }
                // `max(by: severityOrder)`.
                let severity = indices
                    .iter()
                    .map(|index| diagnostics[*index].severity)
                    .reduce(|result, severity| if rank(result) < rank(severity) { severity } else { result })
                    .unwrap_or(DocumentHealthSeverity::Info);
                rows.push(Row::Group(category, severity, indices.len()));
                rows.extend(indices.iter().map(|index| Row::Diagnostic(*index)));
            }
        }
        let count_text = if visible.is_empty() {
            "No issues".to_owned()
        } else {
            format!("{} finding{}", visible.len(), if visible.len() == 1 { "" } else { "s" })
        };
        ivars.count_label.setStringValue(&ns_string(&count_text));
        set_label(&*ivars.count_label, &ivars.count_label.stringValue().to_string());
        ivars.table.reloadData();
        self.restore_selection();
        self.update_empty_state();
        self.reload_preview();
        self.update_action_state();
    }

    /// Reselect the same finding after a reparse rather than dropping the
    /// user's selection — and its preview — on every keystroke.
    fn restore_selection(&self) {
        let ivars = self.ivars();
        let found = (|| {
            let selected_id = ivars.selected_diagnostic_id.borrow().clone()?;
            let index = ivars.diagnostics.borrow().iter().position(|diagnostic| diagnostic.id == selected_id)?;
            if ivars.ignored_ids.borrow().contains(&selected_id) {
                return None;
            }
            let row = ivars.rows.borrow().iter().position(|row| *row == Row::Diagnostic(index))?;
            Some((index, row))
        })();
        let Some((index, row)) = found else {
            unsafe { ivars.table.deselectAll(None) };
            ivars.selected_index.set(None);
            *ivars.selected_diagnostic_id.borrow_mut() = None;
            return;
        };
        ivars.selected_index.set(Some(index));
        ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
    }

    fn update_empty_state(&self) {
        let ivars = self.ivars();
        let scroll = self.scroll();
        if !ivars.rows.borrow().is_empty() {
            ivars.empty_state.setHidden(true);
            scroll.setHidden(false);
            return;
        }
        let subtitle = if ivars.diagnostics.borrow().is_empty() {
            "Headings, links, and structure all\nlook right from here."
        } else {
            "Every finding in this document\nhas been ignored."
        };
        let style_sheet = self.style_sheet();
        ivars.empty_state.configure("checkmark.seal", "No findings", subtitle, &style_sheet);
        ivars.empty_state.setHidden(false);
        scroll.setHidden(true);
    }

    fn diagnostic_index(&self, row: isize) -> Option<usize> {
        let ivars = self.ivars();
        let rows = ivars.rows.borrow();
        if !(row >= 0 && (row as usize) < rows.len()) {
            return None;
        }
        match rows[row as usize] {
            Row::Diagnostic(index) if index < ivars.diagnostics.borrow().len() => Some(index),
            _ => None,
        }
    }

    fn activate_selection(&self) {
        let ivars = self.ivars();
        let clicked = ivars.table.clickedRow();
        let row = if clicked >= 0 { clicked } else { ivars.table.selectedRow() };
        let Some(index) = self.diagnostic_index(row) else { return };
        ivars.selected_index.set(Some(index));
        let diagnostic = ivars.diagnostics.borrow()[index].clone();
        *ivars.selected_diagnostic_id.borrow_mut() = Some(diagnostic.id.clone());
        *ivars.action_status.borrow_mut() = None;
        self.reload_preview();
        self.update_action_state();
        if let Some(delegate) = self.delegate() {
            delegate.document_health_view_did_select(self, &diagnostic);
        }
    }

    /// Every fix this button would apply, in the order they will be written.
    fn safe_fixes(&self) -> Vec<TextEdit> {
        let ivars = self.ivars();
        let length = swift::utf16_count(&ivars.source_text.borrow());
        let visible = self.visible_diagnostics();
        let diagnostics = ivars.diagnostics.borrow();
        let fixes: Vec<TextEdit> = visible
            .iter()
            .filter_map(|index| {
                let fix = diagnostics[*index].fix.as_ref()?;
                if !(fix.range.location >= 0 && fix.range.location + fix.range.length <= length) {
                    return None;
                }
                Some(fix.clone())
            })
            .collect();
        non_overlapping(fixes)
    }

    fn apply_safe_fixes(&self) {
        let fixes = self.safe_fixes();
        if fixes.is_empty() {
            return;
        }
        let ivars = self.ivars();
        // Say what happened. Rows vanishing is not feedback (§11.4).
        *ivars.action_status.borrow_mut() =
            Some(format!("Applied {} safe fix{}.", fixes.len(), if fixes.len() == 1 { "" } else { "es" }));
        ivars.is_applying_fixes.set(true);
        if let Some(delegate) = self.delegate() {
            delegate.document_health_view_did_apply(self, &fixes);
        }
        self.reload_preview();
        ivars.is_applying_fixes.set(false);
    }

    fn ignore_selection(&self) {
        let ivars = self.ivars();
        let Some(selected_index) = ivars.selected_index.get() else { return };
        if !(selected_index < ivars.diagnostics.borrow().len()) {
            return;
        }
        let ignored = ivars.diagnostics.borrow()[selected_index].clone();
        ivars.ignored_ids.borrow_mut().insert(ignored.id.clone());
        *ivars.selected_diagnostic_id.borrow_mut() = None;
        *ivars.action_status.borrow_mut() = Some(format!("Ignored \u{201C}{}\u{201D}.", ignored.message));
        ivars.is_applying_fixes.set(true);
        self.reload();
        ivars.is_applying_fixes.set(false);
    }

    /// Shows the diff box only when there is a diff. Offering to "select a
    /// finding" over an empty list is an instruction the panel cannot honour.
    fn reload_preview(&self) {
        let ivars = self.ivars();
        let action_status = ivars.action_status.borrow().clone();
        if let Some(action_status) = action_status {
            ivars.preview_label.setStringValue(&ns_string(&action_status));
            self.set_preview_visible(false);
            return;
        }
        if ivars.rows.borrow().is_empty() {
            ivars.preview_label.setStringValue(&ns_string(""));
            self.set_preview_visible(false);
            return;
        }
        let source_text = ivars.source_text.borrow().clone();
        let fix = ivars.selected_index.get().and_then(|selected_index| {
            let diagnostics = ivars.diagnostics.borrow();
            if !(selected_index < diagnostics.len()) {
                return None;
            }
            let fix = diagnostics[selected_index].fix.clone()?;
            if !(fix.range.location >= 0 && fix.range.location + fix.range.length <= swift::utf16_count(&source_text)) {
                return None;
            }
            Some(fix)
        });
        let Some(fix) = fix else {
            ivars.preview_label.setStringValue(&ns_string("Select a finding to preview a safe source change."));
            self.set_preview_visible(false);
            return;
        };
        let before = substring(&source_text, fix.range);
        ivars.preview_label.setStringValue(&ns_string(&format!("Preview \u{00B7} {}", fix.summary)));
        ivars.preview.setString(&ns_string(&format!("\u{2212} {before}\n+ {}", fix.replacement)));
        set_label(&*ivars.preview, &format!("Source change preview: replace {before} with {}", fix.replacement));
        self.set_preview_visible(true);
    }

    fn set_preview_visible(&self, visible: bool) {
        let ivars = self.ivars();
        if ivars.preview.isHidden() != visible {
            return;
        }
        ivars.preview.setHidden(!visible);
        if !visible {
            ivars.preview.setString(&ns_string(""));
        }
        let constraints = ivars.preview_constraints.borrow().clone();
        for constraint in constraints {
            constraint.setConstant(if visible { 74.0 } else { 0.0 });
        }
    }

    fn update_action_state(&self) {
        let ivars = self.ivars();
        let count = self.safe_fixes().len();
        // The button says how much it will do before it does it (§11.4).
        let title = if count == 1 { "Apply 1 Safe Fix".to_owned() } else { format!("Apply {count} Safe Fixes") };
        ivars.apply_button.setTitle(&ns_string(&title));
        ivars.apply_button.setEnabled(count > 0);
        set_label(&*ivars.apply_button, &ivars.apply_button.title().to_string());
        ivars.ignore_button.setEnabled(ivars.selected_index.get().is_some());
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.count_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.preview_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.preview.setBackgroundColor(&style_sheet.background);
        ivars.preview.setTextColor(Some(&style_sheet.text));
        ivars.table.reloadData();
        self.update_empty_state();
    }

    // MARK: NSTableViewDataSource, NSTableViewDelegate

    fn height_of_row(&self, row: isize) -> CGFloat {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return PanelMetrics::WIDE_ROW_HEIGHT;
        }
        if let Row::Group(..) = rows[row as usize] {
            return PanelMetrics::GROUP_ROW_HEIGHT;
        }
        PanelMetrics::WIDE_ROW_HEIGHT
    }

    fn is_group_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        matches!(rows[row as usize], Row::Group(..))
    }

    fn should_select_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        !matches!(rows[row as usize], Row::Group(..))
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let ivars = self.ivars();
        let entry = {
            let rows = ivars.rows.borrow();
            if !(row < rows.len() as isize) {
                return None;
            }
            rows[row as usize]
        };
        match entry {
            Row::Group(category, severity, count) => {
                let id = NSString::from_str("documentHealthGroup");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<PanelGroupRowView>(&view))
                    .unwrap_or_else(|| PanelGroupRowView::new(&id, self.mtm()));
                let text = format!("{}  \u{00B7}  {count}", swift::uppercased(category.raw_value()));
                cell.configure(&text, &self.color(severity));
                Some(Retained::into_super(cell))
            }
            Row::Diagnostic(index) => {
                let diagnostic = {
                    let diagnostics = ivars.diagnostics.borrow();
                    if !(index < diagnostics.len()) {
                        return None;
                    }
                    diagnostics[index].clone()
                };
                let id = NSString::from_str("documentHealthRow");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<HealthDiagnosticRowView>(&view))
                    .unwrap_or_else(|| HealthDiagnosticRowView::new(&id, self.mtm()));
                let line_index = ivars.line_index.borrow().clone();
                cell.configure(&diagnostic, &line_index.caption(diagnostic.range), &self.style_sheet());
                Some(Retained::into_super(cell))
            }
        }
    }

    fn color(&self, severity: DocumentHealthSeverity) -> Retained<NSColor> {
        let style_sheet = self.style_sheet();
        match severity {
            DocumentHealthSeverity::Error => style_sheet.callout_color(CalloutKind::Danger),
            DocumentHealthSeverity::Warning => style_sheet.callout_color(CalloutKind::Warning),
            DocumentHealthSeverity::Info => style_sheet.text_faint.clone(),
        }
    }
}

fn rank(severity: DocumentHealthSeverity) -> i32 {
    match severity {
        DocumentHealthSeverity::Info => 0,
        DocumentHealthSeverity::Warning => 1,
        DocumentHealthSeverity::Error => 2,
    }
}

/// `nonOverlapping(_:)`: sorted by descending start (Swift's sort is stable,
/// as Rust's is), then every edit that ends after the previous one starts is
/// dropped.
fn non_overlapping(mut edits: Vec<TextEdit>) -> Vec<TextEdit> {
    let mut last_start = isize::MAX;
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.location));
    edits
        .into_iter()
        .filter(|edit| {
            if !(edit.range.location + edit.range.length <= last_start) {
                return false;
            }
            last_start = edit.range.location;
            true
        })
        .collect()
}

/// `(text as NSString).substring(with: range)`, bridged back to a Swift
/// `String`.
fn substring(text: &str, range: upleft_core::NSRange) -> String {
    let units = swift::ns::utf16(text);
    let start = range.location as usize;
    let end = (range.location + range.length) as usize;
    swift::ns::string_from_utf16(&units[start..end])
}

/// `NSStackView(views:)`.
fn stack_view(views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
    NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm)
}

// MARK: - HealthDiagnosticRowView

pub struct HealthDiagnosticRowViewIvars {
    message_label: Retained<NSTextField>,
    reason_label: Retained<NSTextField>,
    range_label: Retained<NSTextField>,
}

define_class!(
    /// `HealthDiagnosticRowView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "HealthDiagnosticRowView"]
    #[ivars = HealthDiagnosticRowViewIvars]
    pub struct HealthDiagnosticRowView;

    unsafe impl NSObjectProtocol for HealthDiagnosticRowView {}
);

impl HealthDiagnosticRowView {
    /// `init(identifier:)`.
    pub fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<HealthDiagnosticRowView> {
        let this = Self::alloc(mtm).set_ivars(HealthDiagnosticRowViewIvars {
            message_label: label("", mtm),
            reason_label: label("", mtm),
            range_label: label("", mtm),
        });
        let this: Retained<HealthDiagnosticRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        let ivars = this.ivars();
        for label in [&ivars.message_label, &ivars.reason_label, &ivars.range_label] {
            label.setTranslatesAutoresizingMaskIntoConstraints(false);
            label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            this.addSubview(label);
        }
        ivars.message_label.setFont(Some(&PanelFont::row_emphasised()));
        ivars.reason_label.setFont(Some(&PanelFont::secondary()));
        ivars.range_label.setFont(Some(&PanelFont::secondary()));
        let (message, reason, range) = (&ivars.message_label, &ivars.reason_label, &ivars.range_label);
        activate(&[
            message.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            message.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            message.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 6.0),
            reason.leadingAnchor().constraintEqualToAnchor(&message.leadingAnchor()),
            reason.trailingAnchor().constraintEqualToAnchor(&message.trailingAnchor()),
            reason.topAnchor().constraintEqualToAnchor_constant(&message.bottomAnchor(), 2.0),
            range.leadingAnchor().constraintEqualToAnchor(&message.leadingAnchor()),
            range.trailingAnchor().constraintEqualToAnchor(&message.trailingAnchor()),
            range.topAnchor().constraintEqualToAnchor_constant(&reason.bottomAnchor(), 2.0),
        ]);
        set_role(&*this, role::button());
        this
    }

    pub fn configure(&self, diagnostic: &DocumentHealthDiagnostic, line_caption: &str, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        let message = format!("{}: {}", swift::capitalized(diagnostic.severity.raw_value()), diagnostic.message);
        ivars.message_label.setStringValue(&ns_string(&message));
        ivars.reason_label.setStringValue(&ns_string(&diagnostic.explanation));
        // A reader locates a finding by line. VoiceOver keeps the exact
        // source range, which is the one place the byte offsets are still
        // useful.
        ivars.range_label.setStringValue(&ns_string(line_caption));
        ivars.message_label.setTextColor(Some(&Self::color(diagnostic.severity, style_sheet)));
        ivars.reason_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.range_label.setTextColor(Some(&style_sheet.text_faint));
        self.setToolTip(Some(&ns_string(&format!("{}\n{}", diagnostic.message, diagnostic.explanation))));
        let range = format!(
            "Source range {}\u{2013}{}",
            diagnostic.range.location,
            diagnostic.range.location + diagnostic.range.length
        );
        let message_text = ivars.message_label.stringValue().to_string();
        let reason_text = ivars.reason_label.stringValue().to_string();
        set_label(self, &format!("{message_text}. {reason_text}. {line_caption}. {range}"));
    }

    fn color(severity: DocumentHealthSeverity, style_sheet: &StyleSheet) -> Retained<NSColor> {
        match severity {
            DocumentHealthSeverity::Error => style_sheet.callout_color(CalloutKind::Danger),
            DocumentHealthSeverity::Warning => style_sheet.callout_color(CalloutKind::Warning),
            DocumentHealthSeverity::Info => style_sheet.text_secondary.clone(),
        }
    }
}
