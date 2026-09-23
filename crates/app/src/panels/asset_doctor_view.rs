//! Port of `Panels/AssetDoctorView.swift`: a local, source-first report for
//! image references.
//!
//! The view does not resolve URLs, read files, or contact the network. The
//! host supplies the diagnostics and owns every edit. This keeps the panel
//! safe to open while a document is dirty and makes every source change pass
//! through the document's normal undo path.
//!
//! Objective-C class names: `AssetDoctorView`, `AssetDiagnosticRowView`.

// `!(a < b)` spells Swift's `guard a < b`; the negated comparisons are
// deliberate.
#![allow(clippy::nonminimal_bool)]

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlTextEditingDelegate, NSEvent, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSLineBreakMode, NSMenu, NSMenuItem, NSResponder,
    NSScrollView, NSTableColumn, NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate,
    NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSUserInterfaceItemIdentification, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSPoint, NSRect, NSString};
use upleft_render::appkit_compat::RectExt;
use upleft_render::core_types::CalloutKind;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role, weight_bold};
use super::panel_chrome::{
    ButtonAction, PanelBackdrop, PanelButton, PanelEmptyStateView, PanelFont, PanelGroupRowView, PanelList,
    PanelMetrics, PanelSurface, PanelTableView, install_backdrop, panel_title, refresh_tracking_area,
};
use crate::assets::asset_doctor::{
    AssetDiagnostic, AssetDiagnosticCode, AssetDiagnosticSeverity, AssetProposalKind, AssetSourceProposal,
};
use crate::support::commands::Command;

/// `AssetDoctorViewDelegate`.
pub trait AssetDoctorViewDelegate {
    fn asset_doctor_view_did_select(&self, view: &AssetDoctorView, diagnostic: &AssetDiagnostic);
    fn asset_doctor_view_did_reveal(&self, view: &AssetDoctorView, diagnostic: &AssetDiagnostic);
    fn asset_doctor_view_did_request_proposal(
        &self,
        view: &AssetDoctorView,
        kind: AssetProposalKind,
        diagnostic: &AssetDiagnostic,
    );
    fn asset_doctor_view_did_apply(&self, view: &AssetDoctorView, proposal: &AssetSourceProposal);
}

/// `AssetDoctorView.Row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Group { code: AssetDiagnosticCode, severity: AssetDiagnosticSeverity, count: usize },
    Diagnostic(usize),
}

pub struct AssetDoctorViewIvars {
    delegate: RefCell<Option<Weak<dyn AssetDoctorViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    diagnostics: RefCell<Vec<AssetDiagnostic>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
    empty_state: Retained<PanelEmptyStateView>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`.
    scroll: OnceCell<Retained<NSScrollView>>,
    rows: RefCell<Vec<Row>>,
}

define_class!(
    /// `AssetDoctorView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "AssetDoctorView"]
    #[ivars = AssetDoctorViewIvars]
    pub struct AssetDoctorView;

    unsafe impl NSObjectProtocol for AssetDoctorView {}

    impl AssetDoctorView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::DETAIL_WIDTH
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }

        #[unsafe(method(rowClicked:))]
        fn __row_clicked(&self, _sender: Option<&AnyObject>) {
            if !(self.ivars().table.clickedRow() >= 0) {
                return;
            }
            self.activate_selection();
        }
    }

    unsafe impl NSTableViewDataSource for AssetDoctorView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for AssetDoctorView {}

    unsafe impl NSTableViewDelegate for AssetDoctorView {
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

impl PanelSurface for AssetDoctorView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl AssetDoctorView {
    /// `AssetDoctorView()`: `init(styleSheet: .current)`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<AssetDoctorView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<AssetDoctorView> {
        // Stored property initial values, in declaration order.
        let title_label = label(&panel_title(Command::AssetDoctor), mtm);
        let status_label = label("", mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let table = PanelList::make_table_view("assetDoctor", mtm);
        // The initialiser body before `super.init`.
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(AssetDoctorViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            diagnostics: RefCell::new(Vec::new()),
            backdrop,
            title_label,
            status_label,
            empty_state,
            table,
            scroll: OnceCell::new(),
            rows: RefCell::new(Vec::new()),
        });
        let this: Retained<AssetDoctorView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.finish_init();
        this
    }

    fn finish_init(&self) {
        let ivars = self.ivars();
        install_backdrop(self, &ivars.backdrop);

        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::header()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);

        let status_label = &ivars.status_label;
        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setAlignment(NSTextAlignment::Right);
        status_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        // The header must give way before it runs off a narrow inspector.
        status_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        title_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow + 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        status_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(status_label);

        let table = &ivars.table;
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
            table.setTarget(Some(object(self)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<AssetDoctorView> = ObjcWeak::from(self);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&**table, "Asset diagnostics");
        let scroll = self.scroll();
        self.addSubview(&scroll);
        ivars.empty_state.install(self, &scroll, 1.0);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            status_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            status_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            status_label
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 6.0),
            scroll.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);

        set_role(self, role::group());
        set_label(self, "Asset Doctor");
        self.apply_style();
        self.reload();
    }

    /// Swift's `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self) -> Retained<NSScrollView> {
        let ivars = self.ivars();
        ivars.scroll.get_or_init(|| PanelList::make_scroll_view(&ivars.table, self.mtm())).clone()
    }

    // MARK: Properties

    pub fn delegate(&self) -> Option<Rc<dyn AssetDoctorViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn AssetDoctorViewDelegate>>) {
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

    pub fn diagnostics(&self) -> Vec<AssetDiagnostic> {
        self.ivars().diagnostics.borrow().clone()
    }

    pub fn set_diagnostics(&self, diagnostics: Vec<AssetDiagnostic>) {
        *self.ivars().diagnostics.borrow_mut() = diagnostics;
        self.reload();
    }

    pub fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }

    pub fn reload(&self) {
        self.rebuild_rows();
        self.ivars().table.reloadData();
        self.update_status();
        self.update_empty_state();
    }

    /// "No issues" in a corner over an empty list is not an answer. Every
    /// image resolving is a *result*, and the panel should say so (§11.4).
    fn update_empty_state(&self) {
        let ivars = self.ivars();
        let scroll = self.scroll();
        if !ivars.rows.borrow().is_empty() {
            ivars.empty_state.setHidden(true);
            scroll.setHidden(false);
            return;
        }
        let style_sheet = self.style_sheet();
        ivars.empty_state.configure(
            "photo.on.rectangle",
            "Every image resolves",
            "No missing files, unsafe destinations,\nor absolute paths in this document.",
            &style_sheet,
        );
        ivars.empty_state.setHidden(false);
        scroll.setHidden(true);
    }

    fn rebuild_rows(&self) {
        let ivars = self.ivars();
        let diagnostics = ivars.diagnostics.borrow();
        let mut rows = ivars.rows.borrow_mut();
        rows.clear();
        // `Dictionary(grouping:)`: each group keeps the indices' order.
        let mut grouped: HashMap<AssetDiagnosticCode, Vec<usize>> = HashMap::new();
        for (index, diagnostic) in diagnostics.iter().enumerate() {
            grouped.entry(diagnostic.code).or_default().push(index);
        }
        let mut codes: Vec<AssetDiagnosticCode> = grouped.keys().copied().collect();
        codes.sort_by(|a, b| a.raw_value().cmp(b.raw_value()));
        for code in codes {
            let Some(indices) = grouped.get(&code) else { continue };
            if indices.is_empty() {
                continue;
            }
            // `max(by: severityOrder)`.
            let severity = indices
                .iter()
                .map(|index| diagnostics[*index].severity)
                .reduce(|result, severity| if severity_rank(result) < severity_rank(severity) { severity } else { result })
                .unwrap_or(AssetDiagnosticSeverity::Info);
            rows.push(Row::Group { code, severity, count: indices.len() });
            rows.extend(indices.iter().map(|index| Row::Diagnostic(*index)));
        }
    }

    fn update_status(&self) {
        let ivars = self.ivars();
        let text = {
            let diagnostics = ivars.diagnostics.borrow();
            if diagnostics.is_empty() {
                "No issues".to_owned()
            } else {
                let errors = diagnostics.iter().filter(|diagnostic| diagnostic.severity == AssetDiagnosticSeverity::Error).count();
                let warnings =
                    diagnostics.iter().filter(|diagnostic| diagnostic.severity == AssetDiagnosticSeverity::Warning).count();
                let parts: Vec<String> = [
                    (errors > 0).then(|| format!("{errors} error{}", if errors == 1 { "" } else { "s" })),
                    (warnings > 0).then(|| format!("{warnings} warning{}", if warnings == 1 { "" } else { "s" })),
                ]
                .into_iter()
                .flatten()
                .collect();
                if parts.is_empty() { format!("{} info", diagnostics.len()) } else { parts.join(" \u{00B7} ") }
            }
        };
        ivars.status_label.setStringValue(&ns_string(&text));
        set_label(&*ivars.status_label, &ivars.status_label.stringValue().to_string());
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.status_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.table.reloadData();
        self.update_empty_state();
    }

    fn activate_selection(&self) {
        let ivars = self.ivars();
        let clicked = ivars.table.clickedRow();
        let row = if clicked >= 0 { clicked } else { ivars.table.selectedRow() };
        let Some(index) = self.diagnostic_index(row) else { return };
        let diagnostic = ivars.diagnostics.borrow()[index].clone();
        if let Some(delegate) = self.delegate() {
            delegate.asset_doctor_view_did_select(self, &diagnostic);
        }
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

    /// `fileprivate func requestProposal(_:row:)`.
    fn request_proposal(&self, kind: AssetProposalKind, row: isize) {
        let Some(index) = self.diagnostic_index(row) else { return };
        let diagnostic = self.ivars().diagnostics.borrow()[index].clone();
        if let Some(delegate) = self.delegate() {
            delegate.asset_doctor_view_did_request_proposal(self, kind, &diagnostic);
        }
    }

    /// `fileprivate func reveal(row:)`.
    fn reveal(&self, row: isize) {
        let Some(index) = self.diagnostic_index(row) else { return };
        let diagnostic = self.ivars().diagnostics.borrow()[index].clone();
        if let Some(delegate) = self.delegate() {
            delegate.asset_doctor_view_did_reveal(self, &diagnostic);
        }
    }

    /// Used by a host after it validates a replacement. The panel does not
    /// mutate source text itself; this method only forwards the proposal so
    /// a caller can test the same one-edit path as the buttons.
    pub fn apply(&self, proposal: &AssetSourceProposal) {
        if let Some(delegate) = self.delegate() {
            delegate.asset_doctor_view_did_apply(self, proposal);
        }
    }

    // MARK: NSTableViewDataSource, NSTableViewDelegate

    fn height_of_row(&self, row: isize) -> CGFloat {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return PanelMetrics::DETAIL_ROW_HEIGHT;
        }
        if let Row::Group { .. } = rows[row as usize] {
            return PanelMetrics::GROUP_ROW_HEIGHT;
        }
        PanelMetrics::DETAIL_ROW_HEIGHT
    }

    fn is_group_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        matches!(rows[row as usize], Row::Group { .. })
    }

    fn should_select_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        !matches!(rows[row as usize], Row::Group { .. })
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
            Row::Group { code, severity, count } => {
                let identifier = NSString::from_str("assetDoctorGroup");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<PanelGroupRowView>(&view))
                    .unwrap_or_else(|| PanelGroupRowView::new(&identifier, self.mtm()));
                let text = format!("{}  \u{00B7}  {count}", swift::uppercased(Self::label(code)));
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
                let identifier = NSString::from_str("assetDoctorRow");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<AssetDiagnosticRowView>(&view))
                    .unwrap_or_else(|| AssetDiagnosticRowView::new(&identifier, self.mtm()));
                let weak: ObjcWeak<AssetDoctorView> = ObjcWeak::from(self);
                let on_select: Rc<dyn Fn()> = Rc::new(move || {
                    let Some(this) = weak.load() else { return };
                    let diagnostic = {
                        let diagnostics = this.ivars().diagnostics.borrow();
                        if !(index < diagnostics.len()) {
                            return;
                        }
                        diagnostics[index].clone()
                    };
                    if let Some(delegate) = this.delegate() {
                        delegate.asset_doctor_view_did_select(&this, &diagnostic);
                    }
                });
                let weak: ObjcWeak<AssetDoctorView> = ObjcWeak::from(self);
                let on_reveal: Rc<dyn Fn()> = Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.reveal(row);
                    }
                });
                let weak: ObjcWeak<AssetDoctorView> = ObjcWeak::from(self);
                let on_relink: Rc<dyn Fn()> = Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.request_proposal(AssetProposalKind::Relink, row);
                    }
                });
                let weak: ObjcWeak<AssetDoctorView> = ObjcWeak::from(self);
                let on_rename: Rc<dyn Fn()> = Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.request_proposal(AssetProposalKind::Rename, row);
                    }
                });
                cell.configure(&diagnostic, &self.style_sheet(), on_select, on_reveal, on_relink, on_rename);
                Some(Retained::into_super(cell))
            }
        }
    }

    fn label(code: AssetDiagnosticCode) -> &'static str {
        match code {
            AssetDiagnosticCode::Missing => "Missing assets",
            AssetDiagnosticCode::OutsideWorkspace => "Outside workspace",
            AssetDiagnosticCode::AbsolutePath => "Absolute paths",
            AssetDiagnosticCode::Duplicate => "Duplicate assets",
            AssetDiagnosticCode::UnsupportedFormat => "Unsupported formats",
            AssetDiagnosticCode::LargeFile => "Large files",
            AssetDiagnosticCode::MissingAlt => "Missing alt text",
            AssetDiagnosticCode::Unsafe => "Unsafe destinations",
            AssetDiagnosticCode::Malformed => "Malformed destinations",
        }
    }

    fn color(&self, severity: AssetDiagnosticSeverity) -> Retained<NSColor> {
        let style_sheet = self.style_sheet();
        match severity {
            AssetDiagnosticSeverity::Error => style_sheet.callout_color(CalloutKind::Danger),
            AssetDiagnosticSeverity::Warning => style_sheet.callout_color(CalloutKind::Warning),
            AssetDiagnosticSeverity::Info => style_sheet.text_faint.clone(),
        }
    }
}

fn severity_rank(severity: AssetDiagnosticSeverity) -> i32 {
    match severity {
        AssetDiagnosticSeverity::Info => 0,
        AssetDiagnosticSeverity::Warning => 1,
        AssetDiagnosticSeverity::Error => 2,
    }
}

// MARK: - AssetDiagnosticRowView

/// `AssetDiagnosticRowView.Handlers`.
struct Handlers {
    select: Rc<dyn Fn()>,
    reveal: Rc<dyn Fn()>,
    relink: Rc<dyn Fn()>,
    rename: Rc<dyn Fn()>,
}

pub struct AssetDiagnosticRowViewIvars {
    reduce_motion: Cell<bool>,
    severity_label: Retained<NSTextField>,
    message_label: Retained<NSTextField>,
    line_label: Retained<NSTextField>,
    actions_button: Retained<NSButton>,
    actions_action: RefCell<Option<Retained<ButtonAction>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    handlers: RefCell<Option<Handlers>>,
}

define_class!(
    /// One asset finding (`AssetDiagnosticRowView`, private in Swift).
    ///
    /// The row's four actions live in a menu rather than four bordered
    /// buttons. Four bezels per row in a list of twenty is the loudest thing
    /// in a low-chrome app, and at 54pt they clipped anyway. The menu is on
    /// the row's context menu (pointer) and behind one quiet glyph that
    /// appears on hover, so every action still has a pointer path and a
    /// keyboard path (§11.3).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "AssetDiagnosticRowView"]
    #[ivars = AssetDiagnosticRowViewIvars]
    pub struct AssetDiagnosticRowView;

    unsafe impl NSObjectProtocol for AssetDiagnosticRowView {}

    impl AssetDiagnosticRowView {
        #[unsafe(method(performAction:))]
        fn __perform_action(&self, sender: &NSMenuItem) {
            self.perform_action(sender);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveInActiveApp
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.set_actions_visible(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.set_actions_visible(true);
        }
    }
);

impl AssetDiagnosticRowView {
    /// `init(identifier:)`.
    pub fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<AssetDiagnosticRowView> {
        // Stored property initial values, then the body before `super.init`.
        let severity_label = label("", mtm);
        let message_label = label("", mtm);
        let line_label = label("", mtm);
        let actions_button = PanelButton::symbol_default("ellipsis.circle", "Asset actions", &ButtonAction::noop(mtm), mtm);
        let this = Self::alloc(mtm).set_ivars(AssetDiagnosticRowViewIvars {
            reduce_motion: Cell::new(false),
            severity_label,
            message_label,
            line_label,
            actions_button,
            actions_action: RefCell::new(None),
            tracking_area: RefCell::new(None),
            handlers: RefCell::new(None),
        });
        let this: Retained<AssetDiagnosticRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        let ivars = this.ivars();

        let severity_label = &ivars.severity_label;
        severity_label.setFont(Some(&PanelFont::system(11.0, weight_bold())));
        severity_label.setAlignment(NSTextAlignment::Center);
        severity_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(severity_label);

        let message_label = &ivars.message_label;
        message_label.setFont(Some(&PanelFont::row()));
        message_label.setMaximumNumberOfLines(2);
        message_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        message_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(message_label);

        let line_label = &ivars.line_label;
        line_label.setFont(Some(&PanelFont::secondary()));
        line_label.setAlignment(NSTextAlignment::Right);
        line_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        line_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(line_label);

        let weak: ObjcWeak<AssetDiagnosticRowView> = ObjcWeak::from(&*this);
        let action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.show_actions_menu();
                }
            },
            mtm,
        );
        *ivars.actions_action.borrow_mut() = Some(action.clone());
        let actions_button = &ivars.actions_button;
        unsafe {
            actions_button.setTarget(Some(&action));
            actions_button.setAction(Some(ButtonAction::selector()));
        }
        actions_button.setAlphaValue(1.0);
        this.addSubview(actions_button);

        activate(&[
            severity_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 8.0),
            severity_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 7.0),
            severity_label.widthAnchor().constraintEqualToConstant(16.0),
            message_label.leadingAnchor().constraintEqualToAnchor_constant(&severity_label.trailingAnchor(), 4.0),
            message_label.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&line_label.leadingAnchor(), -4.0),
            message_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 6.0),
            message_label.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&this.bottomAnchor(), -5.0),
            line_label.trailingAnchor().constraintEqualToAnchor_constant(&actions_button.leadingAnchor(), -2.0),
            line_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 6.0),
            line_label.widthAnchor().constraintGreaterThanOrEqualToConstant(38.0),
            actions_button.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -4.0),
            actions_button.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            actions_button.widthAnchor().constraintEqualToConstant(24.0),
        ]);
        this
    }

    pub fn configure(
        &self,
        diagnostic: &AssetDiagnostic,
        style_sheet: &StyleSheet,
        on_select: Rc<dyn Fn()>,
        on_reveal: Rc<dyn Fn()>,
        on_relink: Rc<dyn Fn()>,
        on_rename: Rc<dyn Fn()>,
    ) {
        let ivars = self.ivars();
        ivars.reduce_motion.set(style_sheet.reduce_motion);
        let severity = if diagnostic.severity == AssetDiagnosticSeverity::Error { "!" } else { "\u{2022}" };
        ivars.severity_label.setStringValue(&ns_string(severity));
        let severity_color: Retained<NSColor> = match diagnostic.severity {
            AssetDiagnosticSeverity::Error => style_sheet.callout_color(CalloutKind::Danger),
            AssetDiagnosticSeverity::Warning => style_sheet.callout_color(CalloutKind::Warning),
            AssetDiagnosticSeverity::Info => style_sheet.text_faint.clone(),
        };
        ivars.severity_label.setTextColor(Some(&severity_color));
        ivars.message_label.setStringValue(&ns_string(&diagnostic.message));
        ivars.message_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.line_label.setStringValue(&ns_string(&format!("Line {}", diagnostic.reference.line)));
        ivars.line_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.actions_button.setContentTintColor(Some(&style_sheet.text_faint));

        *ivars.handlers.borrow_mut() =
            Some(Handlers { select: on_select, reveal: on_reveal, relink: on_relink, rename: on_rename });
        let menu = self.make_actions_menu();
        unsafe { self.setMenu(Some(&menu)) };

        let destination = &diagnostic.reference.source;
        let severity = swift::capitalized(diagnostic.severity.raw_value());
        set_label(
            self,
            &format!(
                "{severity}, line {}: {}. Asset {destination}",
                diagnostic.reference.line, diagnostic.message
            ),
        );
        self.setToolTip(Some(&ns_string(destination)));
    }

    fn make_actions_menu(&self) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        for (title, key) in [
            ("Select in Document", "select"),
            ("Reveal in Finder", "reveal"),
            ("Relink\u{2026}", "relink"),
            ("Rename\u{2026}", "rename"),
        ] {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &ns_string(title),
                    Some(sel!(performAction:)),
                    &NSString::from_str(""),
                )
            };
            unsafe {
                item.setTarget(Some(object(self)));
                item.setRepresentedObject(Some(&ns_string(key)));
            }
            menu.addItem(&item);
        }
        menu
    }

    fn perform_action(&self, sender: &NSMenuItem) {
        let handler = {
            let handlers = self.ivars().handlers.borrow();
            let Some(handlers) = handlers.as_ref() else { return };
            let Some(key) = sender
                .representedObject()
                .and_then(|object| super::appkit_support::downcast::<NSString>(&object))
            else {
                return;
            };
            match key.to_string().as_str() {
                "select" => handlers.select.clone(),
                "reveal" => handlers.reveal.clone(),
                "relink" => handlers.relink.clone(),
                "rename" => handlers.rename.clone(),
                _ => return,
            }
        };
        handler();
    }

    fn show_actions_menu(&self) {
        let Some(menu) = self.menu() else { return };
        let frame = self.ivars().actions_button.frame();
        menu.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(frame.min_x(), frame.max_y()), Some(self));
    }

    fn set_actions_visible(&self, _visible: bool) {
        self.ivars().actions_button.setAlphaValue(1.0);
    }
}
