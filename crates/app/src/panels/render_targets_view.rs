//! Port of `Panels/RenderTargetsView.swift`: a renderer compatibility
//! report. Profiles are data, so the host can add custom targets without
//! changing this panel or the core analyzer.
//!
//! Objective-C class names: `RenderTargetsView`,
//! `CompatibilityDiagnosticRowView`. The view is its table's data source and
//! delegate, as in Swift.

// `!(a < b)` spells Swift's `guard a < b`; the negated comparisons are
// deliberate.
#![allow(clippy::nonminimal_bool)]

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSControlTextEditingDelegate, NSLayoutAttribute, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLineBreakMode, NSPopUpButton, NSResponder, NSScrollView, NSStackView, NSTableColumn,
    NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextAlignment, NSTextField, NSTextView,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSIndexSet, NSRect, NSSize, NSString};
use upleft_core::compatibility::compatibility_diagnostics::{
    CompatibilityDiagnostic, CompatibilityReport, MarkdownCompatibility,
};
use upleft_core::compatibility::render_target::{MarkdownCapability, RenderTargetProfile};
use upleft_core::contracts::TextEdit;
use upleft_core::model::ParsedDocument;
use upleft_render::appkit_compat::{attributed_string, keys};
use upleft_render::core_types::CalloutKind;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role};
use super::panel_chrome::{
    ButtonAction, PanelBackdrop, PanelButton, PanelEmptyStateView, PanelFont, PanelGroupRowView, PanelList,
    PanelMetrics, PanelSurface, PanelTableView, SourceLineIndex, install_backdrop, panel_title,
};
use crate::support::commands::Command;

/// `RenderTargetsViewDelegate`.
pub trait RenderTargetsViewDelegate {
    fn render_targets_view_did_select_profile(&self, view: &RenderTargetsView, profile: &RenderTargetProfile);
    fn render_targets_view_did_select_diagnostic(&self, view: &RenderTargetsView, diagnostic: &CompatibilityDiagnostic);
    fn render_targets_view_did_apply(&self, view: &RenderTargetsView, fixes: &[TextEdit]);
    fn render_targets_view_wants_source_mode(&self, view: &RenderTargetsView);
}

/// `RenderTargetsView.Row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Group(MarkdownCapability, usize),
    Diagnostic(usize),
}

pub struct RenderTargetsViewIvars {
    delegate: RefCell<Option<Weak<dyn RenderTargetsViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    profiles: RefCell<Vec<RenderTargetProfile>>,
    document: RefCell<Arc<ParsedDocument>>,
    source_text: RefCell<String>,
    selected_profile: RefCell<RenderTargetProfile>,
    report: RefCell<CompatibilityReport>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    count_label: Retained<NSTextField>,
    target_control: Retained<NSPopUpButton>,
    preview_label: Retained<NSTextField>,
    preview: Retained<NSTextView>,
    empty_state: Retained<PanelEmptyStateView>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`.
    scroll: OnceCell<Retained<NSScrollView>>,
    apply_button: Retained<NSButton>,
    source_button: Retained<NSButton>,
    apply_action: RefCell<Option<Retained<ButtonAction>>>,
    source_action: RefCell<Option<Retained<ButtonAction>>>,
    rows: RefCell<Vec<Row>>,
    selected_index: Cell<Option<usize>>,
    /// Selection survives a reparse by diagnostic id rather than row number.
    selected_diagnostic_id: RefCell<Option<String>>,
    line_index: RefCell<Rc<SourceLineIndex>>,
    /// What the last action did, shown where the preview normally is.
    action_status: RefCell<Option<String>>,
    is_applying_fixes: Cell<bool>,
    preview_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
}

define_class!(
    /// `RenderTargetsView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "RenderTargetsView"]
    #[ivars = RenderTargetsViewIvars]
    pub struct RenderTargetsView;

    unsafe impl NSObjectProtocol for RenderTargetsView {}

    impl RenderTargetsView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::DETAIL_WIDTH
        }

        #[unsafe(method(targetChanged:))]
        fn __target_changed(&self, sender: &NSPopUpButton) {
            self.target_changed(sender);
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

    unsafe impl NSTableViewDataSource for RenderTargetsView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for RenderTargetsView {}

    unsafe impl NSTableViewDelegate for RenderTargetsView {
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

impl PanelSurface for RenderTargetsView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl RenderTargetsView {
    /// `RenderTargetsView()`: `init(styleSheet: .current)`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<RenderTargetsView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<RenderTargetsView> {
        // Stored property initial values, in declaration order.
        let profiles = RenderTargetProfile::built_ins();
        let document = ParsedDocument::empty();
        let selected_profile = RenderTargetProfile::git_hub();
        let report = CompatibilityReport::new(RenderTargetProfile::git_hub(), Vec::new());
        let title_label = label(&panel_title(Command::RenderTargets), mtm);
        let count_label = label("", mtm);
        let target_control = NSPopUpButton::new(mtm);
        let preview_label = label("", mtm);
        let preview = NSTextView::new(mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let table = PanelList::make_table_view("renderTargets", mtm);
        // The initialiser body before `super.init`.
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let apply_button = PanelButton::text("Apply safe fixes", &ButtonAction::noop(mtm), false, mtm);
        let source_button = PanelButton::text("Open Source Focus", &ButtonAction::noop(mtm), false, mtm);
        let this = Self::alloc(mtm).set_ivars(RenderTargetsViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            profiles: RefCell::new(profiles),
            document: RefCell::new(document),
            source_text: RefCell::new(String::new()),
            selected_profile: RefCell::new(selected_profile),
            report: RefCell::new(report),
            backdrop,
            title_label,
            count_label,
            target_control,
            preview_label,
            preview,
            empty_state,
            table,
            scroll: OnceCell::new(),
            apply_button,
            source_button,
            apply_action: RefCell::new(None),
            source_action: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
            selected_index: Cell::new(None),
            selected_diagnostic_id: RefCell::new(None),
            line_index: RefCell::new(Rc::new(SourceLineIndex::new(""))),
            action_status: RefCell::new(None),
            is_applying_fixes: Cell::new(false),
            preview_constraints: RefCell::new(Vec::new()),
        });
        let this: Retained<RenderTargetsView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
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

        let target_control = &ivars.target_control;
        target_control.setFont(Some(&PanelFont::secondary()));
        unsafe {
            target_control.setTarget(Some(object(self)));
            target_control.setAction(Some(sel!(targetChanged:)));
        }
        set_label(&**target_control, "Render target");
        target_control.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(target_control);

        let table = &ivars.table;
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
            table.setTarget(Some(object(self)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<RenderTargetsView> = ObjcWeak::from(self);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&**table, "Render target findings");
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
        preview.setTextContainerInset(NSSize::new(6.0, 5.0));
        set_label(&**preview, "Render target source change preview");
        preview.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(preview);

        let weak: ObjcWeak<RenderTargetsView> = ObjcWeak::from(self);
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
        set_label(&*ivars.apply_button, "Apply all safe render target fixes");
        let weak: ObjcWeak<RenderTargetsView> = ObjcWeak::from(self);
        let source = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.render_targets_view_wants_source_mode(&this);
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
        for button in [&ivars.apply_button, &ivars.source_button] {
            button.setContentCompressionResistancePriority_forOrientation(
                NSLayoutPriorityDefaultLow,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }
        let actions = stack_view(&[&ivars.apply_button, &ivars.source_button], mtm);
        actions.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        actions.setAlignment(NSLayoutAttribute::CenterY);
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
            target_control.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            target_control.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 6.0),
            target_control.widthAnchor().constraintEqualToConstant(176.0),
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&target_control.bottomAnchor(), 6.0),
            scroll.heightAnchor().constraintGreaterThanOrEqualToConstant(160.0),
            preview_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            preview_label.trailingAnchor().constraintEqualToAnchor(&count_label.trailingAnchor()),
            preview_label.topAnchor().constraintEqualToAnchor_constant(&scroll.bottomAnchor(), 8.0),
            preview.leadingAnchor().constraintEqualToAnchor(&preview_label.leadingAnchor()),
            preview.trailingAnchor().constraintEqualToAnchor(&preview_label.trailingAnchor()),
            preview.topAnchor().constraintEqualToAnchor_constant(&preview_label.bottomAnchor(), 4.0),
            actions.leadingAnchor().constraintEqualToAnchor(&preview_label.leadingAnchor()),
            actions.trailingAnchor().constraintLessThanOrEqualToAnchor(&preview_label.trailingAnchor()),
            actions.topAnchor().constraintEqualToAnchor_constant(&preview.bottomAnchor(), 7.0),
            actions.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&self.bottomAnchor(), -8.0),
        ]);

        // The diff box only exists while there is a diff to put in it.
        let preview_constraints = vec![preview.heightAnchor().constraintEqualToConstant(74.0)];
        *ivars.preview_constraints.borrow_mut() = preview_constraints.clone();
        activate(&preview_constraints);

        set_role(self, role::group());
        set_label(self, "Render targets");
        self.rebuild_target_control();
        self.apply_style();
        self.reload();
    }

    /// Swift's `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self) -> Retained<NSScrollView> {
        let ivars = self.ivars();
        ivars.scroll.get_or_init(|| PanelList::make_scroll_view(&ivars.table, self.mtm())).clone()
    }

    // MARK: Properties

    pub fn delegate(&self) -> Option<Rc<dyn RenderTargetsViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn RenderTargetsViewDelegate>>) {
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

    pub fn profiles(&self) -> Vec<RenderTargetProfile> {
        self.ivars().profiles.borrow().clone()
    }

    pub fn set_profiles(&self, profiles: Vec<RenderTargetProfile>) {
        *self.ivars().profiles.borrow_mut() = profiles;
        self.rebuild_target_control();
    }

    pub fn document(&self) -> Arc<ParsedDocument> {
        self.ivars().document.borrow().clone()
    }

    pub fn set_document(&self, document: Arc<ParsedDocument>) {
        *self.ivars().document.borrow_mut() = document;
        self.recompute_report();
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

    pub fn selected_profile(&self) -> RenderTargetProfile {
        self.ivars().selected_profile.borrow().clone()
    }

    pub fn set_selected_profile(&self, profile: RenderTargetProfile) {
        let ivars = self.ivars();
        let old_value = ivars.selected_profile.replace(profile.clone());
        if profile == old_value {
            return;
        }
        self.sync_target_control();
        self.recompute_report();
        if let Some(delegate) = self.delegate() {
            delegate.render_targets_view_did_select_profile(self, &profile);
        }
    }

    pub fn report(&self) -> CompatibilityReport {
        self.ivars().report.borrow().clone()
    }

    pub fn set_report(&self, report: CompatibilityReport) {
        *self.ivars().report.borrow_mut() = report;
        self.reload();
    }

    pub fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
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

    fn recompute_report(&self) {
        let ivars = self.ivars();
        let document = ivars.document.borrow().clone();
        if !(document.length > 0 || !document.text.is_empty()) {
            return;
        }
        let profile = ivars.selected_profile.borrow().clone();
        self.set_report(MarkdownCompatibility::diagnose(&document, &profile));
    }

    fn rebuild_target_control(&self) {
        let ivars = self.ivars();
        ivars.target_control.removeAllItems();
        let names: Vec<Retained<NSString>> = ivars.profiles.borrow().iter().map(|profile| ns_string(&profile.name)).collect();
        ivars.target_control.addItemsWithTitles(&NSArray::from_retained_slice(&names));
        self.sync_target_control();
    }

    fn sync_target_control(&self) {
        let ivars = self.ivars();
        let index = {
            let selected = ivars.selected_profile.borrow();
            ivars.profiles.borrow().iter().position(|profile| *profile == *selected)
        };
        let Some(index) = index else { return };
        ivars.target_control.selectItemAtIndex(index as isize);
        self.theme_target_control();
    }

    fn theme_target_control(&self) {
        let style_sheet = self.style_sheet();
        let control = &self.ivars().target_control;
        control.setContentTintColor(Some(&style_sheet.text));
        let Some(title) = control.selectedItem().map(|item| item.title().to_string()) else { return };
        let font = control.font().unwrap_or_else(PanelFont::row);
        let title = attributed_string(&title, &[(keys::foreground_color(), object(&*style_sheet.text)), (keys::font(), object(&*font))]);
        control.setAttributedTitle(&title);
    }

    fn target_changed(&self, sender: &NSPopUpButton) {
        let index = sender.indexOfSelectedItem();
        let profile = {
            let profiles = self.ivars().profiles.borrow();
            if !(index >= 0 && (index as usize) < profiles.len()) {
                return;
            }
            profiles[index as usize].clone()
        };
        self.set_selected_profile(profile);
    }

    fn reload(&self) {
        let ivars = self.ivars();
        if !ivars.is_applying_fixes.get() {
            *ivars.action_status.borrow_mut() = None;
        }
        let diagnostic_count = {
            let report = ivars.report.borrow();
            let mut rows = ivars.rows.borrow_mut();
            rows.clear();
            // `Dictionary(grouping:by:)`: each group keeps the indices' order.
            let mut grouped: HashMap<MarkdownCapability, Vec<usize>> = HashMap::new();
            for (index, diagnostic) in report.diagnostics.iter().enumerate() {
                grouped.entry(diagnostic.capability).or_default().push(index);
            }
            for capability in MarkdownCapability::ALL_CASES {
                let Some(indices) = grouped.get(&capability) else { continue };
                if indices.is_empty() {
                    continue;
                }
                rows.push(Row::Group(capability, indices.len()));
                rows.extend(indices.iter().map(|index| Row::Diagnostic(*index)));
            }
            report.diagnostics.len()
        };
        let count_text = if diagnostic_count == 0 {
            "Compatible".to_owned()
        } else {
            format!("{diagnostic_count} issue{}", if diagnostic_count == 1 { "" } else { "s" })
        };
        ivars.count_label.setStringValue(&ns_string(&count_text));
        set_label(&*ivars.count_label, &ivars.count_label.stringValue().to_string());
        ivars.table.reloadData();
        self.restore_selection();
        self.update_empty_state();
        self.reload_preview();
        self.update_action_state();
    }

    /// Reselect the same finding after a reparse instead of clearing the
    /// selection — and the preview — on every keystroke in the document.
    fn restore_selection(&self) {
        let ivars = self.ivars();
        let found = (|| {
            let selected_id = ivars.selected_diagnostic_id.borrow().clone()?;
            let index = ivars.report.borrow().diagnostics.iter().position(|diagnostic| diagnostic.id == selected_id)?;
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
        let title = format!("Compatible with {}", ivars.selected_profile.borrow().name);
        let style_sheet = self.style_sheet();
        ivars.empty_state.configure(
            "checkmark.seal",
            &title,
            "Everything in this document renders\non the selected target.",
            &style_sheet,
        );
        ivars.empty_state.setHidden(false);
        scroll.setHidden(true);
    }

    fn update_action_state(&self) {
        let ivars = self.ivars();
        let count = self.safe_edits().len();
        let title = if count == 1 { "Apply 1 Safe Fix".to_owned() } else { format!("Apply {count} Safe Fixes") };
        ivars.apply_button.setTitle(&ns_string(&title));
        ivars.apply_button.setEnabled(count > 0);
        set_label(&*ivars.apply_button, &ivars.apply_button.title().to_string());
    }

    fn diagnostic_index(&self, row: isize) -> Option<usize> {
        let ivars = self.ivars();
        let rows = ivars.rows.borrow();
        if !(row >= 0 && (row as usize) < rows.len()) {
            return None;
        }
        match rows[row as usize] {
            Row::Diagnostic(index) if index < ivars.report.borrow().diagnostics.len() => Some(index),
            _ => None,
        }
    }

    fn activate_selection(&self) {
        let ivars = self.ivars();
        let clicked = ivars.table.clickedRow();
        let row = if clicked >= 0 { clicked } else { ivars.table.selectedRow() };
        let Some(index) = self.diagnostic_index(row) else { return };
        ivars.selected_index.set(Some(index));
        let diagnostic = ivars.report.borrow().diagnostics[index].clone();
        *ivars.selected_diagnostic_id.borrow_mut() = Some(diagnostic.id.clone());
        *ivars.action_status.borrow_mut() = None;
        self.reload_preview();
        if let Some(delegate) = self.delegate() {
            delegate.render_targets_view_did_select_diagnostic(self, &diagnostic);
        }
    }

    /// Every edit the apply button would write, in the order it will write
    /// them.
    fn safe_edits(&self) -> Vec<TextEdit> {
        let ivars = self.ivars();
        let length = swift::utf16_count(&ivars.source_text.borrow());
        let report = ivars.report.borrow();
        let edits: Vec<TextEdit> = report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| {
                let proposal = diagnostic.proposal.as_ref()?;
                if !(proposal.range.location >= 0 && proposal.range.location + proposal.range.length <= length) {
                    return None;
                }
                Some(TextEdit::new(proposal.range, proposal.replacement.clone(), proposal.summary.clone(), None))
            })
            .collect();
        non_overlapping(edits)
    }

    fn apply_safe_fixes(&self) {
        let edits = self.safe_edits();
        if edits.is_empty() {
            return;
        }
        let ivars = self.ivars();
        *ivars.action_status.borrow_mut() =
            Some(format!("Applied {} safe fix{}.", edits.len(), if edits.len() == 1 { "" } else { "es" }));
        ivars.is_applying_fixes.set(true);
        if let Some(delegate) = self.delegate() {
            delegate.render_targets_view_did_apply(self, &edits);
        }
        self.reload_preview();
        ivars.is_applying_fixes.set(false);
    }

    /// Shows the diff box only when there is a diff. Asking the reader to
    /// "select a finding" over an empty list is an instruction with no
    /// target.
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
        let proposal = ivars.selected_index.get().and_then(|selected_index| {
            let report = ivars.report.borrow();
            if !(selected_index < report.diagnostics.len()) {
                return None;
            }
            let proposal = report.diagnostics[selected_index].proposal.clone()?;
            if !(proposal.range.location >= 0
                && proposal.range.location + proposal.range.length <= swift::utf16_count(&source_text))
            {
                return None;
            }
            Some(proposal)
        });
        let Some(proposal) = proposal else {
            ivars.preview_label.setStringValue(&ns_string("Select a finding to preview a safe source change."));
            self.set_preview_visible(false);
            return;
        };
        let before = substring(&source_text, proposal.range);
        ivars.preview_label.setStringValue(&ns_string(&format!("Preview \u{00B7} {}", proposal.summary)));
        ivars.preview.setString(&ns_string(&format!("\u{2212} {before}\n+ {}", proposal.replacement)));
        set_label(&*ivars.preview, &format!("Source change preview: replace {before} with {}", proposal.replacement));
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

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.count_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.preview_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.preview.setBackgroundColor(&style_sheet.background);
        ivars.preview.setTextColor(Some(&style_sheet.text));
        self.theme_target_control();
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
            Row::Group(capability, count) => {
                let id = NSString::from_str("renderTargetsGroup");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<PanelGroupRowView>(&view))
                    .unwrap_or_else(|| PanelGroupRowView::new(&id, self.mtm()));
                let text = format!("{}  \u{00B7}  {count}", swift::uppercased(capability.display_name()));
                cell.configure(&text, &self.style_sheet().callout_color(CalloutKind::Warning));
                Some(Retained::into_super(cell))
            }
            Row::Diagnostic(index) => {
                let diagnostic = {
                    let report = ivars.report.borrow();
                    if !(index < report.diagnostics.len()) {
                        return None;
                    }
                    report.diagnostics[index].clone()
                };
                let id = NSString::from_str("renderTargetsRow");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<CompatibilityDiagnosticRowView>(&view))
                    .unwrap_or_else(|| CompatibilityDiagnosticRowView::new(&id, self.mtm()));
                let line_index = ivars.line_index.borrow().clone();
                cell.configure(&diagnostic, &line_index.caption(diagnostic.range), &self.style_sheet());
                Some(Retained::into_super(cell))
            }
        }
    }
}

/// `nonOverlapping(_:)` (private in Swift): sorted by descending start
/// (Swift's sort is stable, as Rust's is), then every edit that ends after
/// the previous one starts is dropped.
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

// MARK: - CompatibilityDiagnosticRowView

pub struct CompatibilityDiagnosticRowViewIvars {
    title_label: Retained<NSTextField>,
    detail_label: Retained<NSTextField>,
    range_label: Retained<NSTextField>,
}

define_class!(
    /// `CompatibilityDiagnosticRowView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "CompatibilityDiagnosticRowView"]
    #[ivars = CompatibilityDiagnosticRowViewIvars]
    pub struct CompatibilityDiagnosticRowView;

    unsafe impl NSObjectProtocol for CompatibilityDiagnosticRowView {}
);

impl CompatibilityDiagnosticRowView {
    /// `init(identifier:)`.
    pub fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<CompatibilityDiagnosticRowView> {
        let this = Self::alloc(mtm).set_ivars(CompatibilityDiagnosticRowViewIvars {
            title_label: label("", mtm),
            detail_label: label("", mtm),
            range_label: label("", mtm),
        });
        let this: Retained<CompatibilityDiagnosticRowView> =
            unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        let ivars = this.ivars();
        for label in [&ivars.title_label, &ivars.detail_label, &ivars.range_label] {
            label.setTranslatesAutoresizingMaskIntoConstraints(false);
            label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            this.addSubview(label);
        }
        ivars.title_label.setFont(Some(&PanelFont::row_emphasised()));
        ivars.detail_label.setFont(Some(&PanelFont::secondary()));
        ivars.range_label.setFont(Some(&PanelFont::secondary()));
        let (title, detail, range) = (&ivars.title_label, &ivars.detail_label, &ivars.range_label);
        activate(&[
            title.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            title.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            title.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 6.0),
            detail.leadingAnchor().constraintEqualToAnchor(&title.leadingAnchor()),
            detail.trailingAnchor().constraintEqualToAnchor(&title.trailingAnchor()),
            detail.topAnchor().constraintEqualToAnchor_constant(&title.bottomAnchor(), 2.0),
            range.leadingAnchor().constraintEqualToAnchor(&title.leadingAnchor()),
            range.trailingAnchor().constraintEqualToAnchor(&title.trailingAnchor()),
            range.topAnchor().constraintEqualToAnchor_constant(&detail.bottomAnchor(), 2.0),
        ]);
        set_role(&*this, role::button());
        this
    }

    pub fn configure(&self, diagnostic: &CompatibilityDiagnostic, line_caption: &str, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        ivars.title_label.setStringValue(&ns_string(&diagnostic.title));
        ivars.detail_label.setStringValue(&ns_string(&diagnostic.explanation));
        // Line for the reader; the exact range stays on the accessibility
        // label, where a precise source position is still worth having.
        ivars.range_label.setStringValue(&ns_string(line_caption));
        ivars.title_label.setTextColor(Some(&style_sheet.callout_color(CalloutKind::Warning)));
        ivars.detail_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.range_label.setTextColor(Some(&style_sheet.text_faint));
        self.setToolTip(Some(&ns_string(&format!("{}\n{}", diagnostic.title, diagnostic.explanation))));
        let range = format!(
            "Source range {}\u{2013}{}",
            diagnostic.range.location,
            diagnostic.range.location + diagnostic.range.length
        );
        set_label(self, &format!("{}. {}. {line_caption}. {range}", diagnostic.title, diagnostic.explanation));
    }
}
