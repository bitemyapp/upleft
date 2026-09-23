//! Port of `Panels/DocumentLensView.swift`: a compact source map for the
//! current document. The panel only owns view state. The host owns parsing,
//! diagnostics, and selection.
//!
//! Objective-C class names: `DocumentLensView`, `DocumentLensRowView`.

// `!(a < b)` spells Swift's `guard a < b`; the negated comparisons are
// deliberate.
#![allow(clippy::nonminimal_bool)]

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSControlTextEditingDelegate, NSLineBreakMode, NSPopUpButton, NSResponder, NSScrollView,
    NSTableColumn, NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextAlignment,
    NSTextField, NSUserInterfaceItemIdentification, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSIndexSet, NSRect, NSString};
use upleft_core::ParsedDocument;
use upleft_core::compatibility::render_target::RenderTargetProfile;
use upleft_render::appkit_compat::{attributed_string, keys};
use upleft_render::core_types::CalloutKind;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role};
use super::panel_chrome::{
    PanelBackdrop, PanelEmptyStateView, PanelFont, PanelGroupRowView, PanelList, PanelMetrics, PanelSurface,
    PanelTableView, SourceLineIndex, element_at, install_backdrop, panel_title,
};
use crate::lens::document_lens_model::{
    DocumentLensGroup, DocumentLensInput, DocumentLensItem, DocumentLensModel, DocumentLensSection,
    DocumentLensSeverity, DocumentLensTab,
};
use crate::support::commands::Command;

/// `DocumentLensViewDelegate`.
pub trait DocumentLensViewDelegate {
    fn document_lens_did_select(&self, view: &DocumentLensView, range: upleft_core::NSRange, item: &DocumentLensItem);
    fn document_lens_did_select_render_target(&self, view: &DocumentLensView, profile: &RenderTargetProfile);
}

/// `DocumentLensView.Row`.
#[derive(Debug, Clone)]
enum Row {
    Group(DocumentLensGroup),
    Item(DocumentLensItem),
}

pub struct DocumentLensViewIvars {
    delegate: RefCell<Option<Weak<dyn DocumentLensViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    model: RefCell<Rc<DocumentLensModel>>,
    /// The document text the model was built from. Supplied so rows can
    /// name a line instead of a byte offset; without it a row simply omits
    /// the position rather than reporting a wrong one.
    source_text: RefCell<String>,
    selected_tab: Cell<DocumentLensTab>,
    render_target_profile: RefCell<RenderTargetProfile>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    count_label: Retained<NSTextField>,
    /// Seven sections cannot fit a segmented control at the inspector's
    /// minimum width — "Structure" and "Render Target" truncated in every
    /// tab. A popup names the section in full and carries its count as well
    /// (§11.4).
    tab_control: Retained<NSPopUpButton>,
    target_control: Retained<NSPopUpButton>,
    empty_state: Retained<PanelEmptyStateView>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`.
    scroll: OnceCell<Retained<NSScrollView>>,
    rows: RefCell<Vec<Row>>,
    /// Selection survives a reload by identity, not by row number: the host
    /// reparses on every keystroke, and losing the selected finding (and its
    /// preview) mid-sentence is the panel forgetting what you were doing.
    selected_item_id: RefCell<Option<String>>,
    line_index: RefCell<Option<Rc<SourceLineIndex>>>,
}

define_class!(
    /// `DocumentLensView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentLensView"]
    #[ivars = DocumentLensViewIvars]
    pub struct DocumentLensView;

    unsafe impl NSObjectProtocol for DocumentLensView {}

    impl DocumentLensView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::DETAIL_WIDTH
        }

        #[unsafe(method(tabChanged:))]
        fn __tab_changed(&self, sender: &NSPopUpButton) {
            let Some(tab) = element_at(&DocumentLensTab::ALL_CASES, sender.indexOfSelectedItem()).copied() else {
                return;
            };
            self.set_selected_tab(tab);
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

    unsafe impl NSTableViewDataSource for DocumentLensView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for DocumentLensView {}

    unsafe impl NSTableViewDelegate for DocumentLensView {
        #[unsafe(method(tableView:heightOfRow:))]
        fn __height_of_row(&self, _table_view: &NSTableView, row: isize) -> CGFloat {
            self.height_of_row(row)
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

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn __row_view_for_row(&self, table_view: &NSTableView, _row: isize) -> Option<Retained<NSTableRowView>> {
            let row_view = PanelList::selection_row(table_view, Some(object(self)), self.style_sheet(), self.mtm());
            Some(Retained::into_super(row_view))
        }
    }
);

impl PanelSurface for DocumentLensView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl DocumentLensView {
    /// `DocumentLensView()`: `init(styleSheet: .current)`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<DocumentLensView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<DocumentLensView> {
        // Stored property initial values, in declaration order.
        let model = DocumentLensModel::new(&DocumentLensInput::new(ParsedDocument::empty()));
        let title_label = label(&panel_title(Command::DocumentLens), mtm);
        let count_label = label("", mtm);
        let tab_control = NSPopUpButton::new(mtm);
        let target_control = NSPopUpButton::new(mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let table = PanelList::make_table_view("documentLens", mtm);
        // The initialiser body before `super.init`.
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(DocumentLensViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            model: RefCell::new(Rc::new(model)),
            source_text: RefCell::new(String::new()),
            selected_tab: Cell::new(DocumentLensTab::Structure),
            render_target_profile: RefCell::new(RenderTargetProfile::git_hub()),
            backdrop,
            title_label,
            count_label,
            tab_control,
            target_control,
            empty_state,
            table,
            scroll: OnceCell::new(),
            rows: RefCell::new(Vec::new()),
            selected_item_id: RefCell::new(None),
            line_index: RefCell::new(None),
        });
        let this: Retained<DocumentLensView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        install_backdrop(&this, &this.ivars().backdrop);

        this.build_header();
        this.build_table();
        this.apply_style();
        this.reload();

        set_role(&*this, role::group());
        set_label(&*this, Command::DocumentLens.title());
        this
    }

    /// Swift's `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self) -> Retained<NSScrollView> {
        let ivars = self.ivars();
        ivars.scroll.get_or_init(|| PanelList::make_scroll_view(&ivars.table, self.mtm())).clone()
    }

    fn build_header(&self) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::header()));
        title_label.setHidden(true);
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);

        let count_label = &ivars.count_label;
        count_label.setFont(Some(&PanelFont::secondary()));
        count_label.setHidden(true);
        count_label.setAlignment(NSTextAlignment::Right);
        count_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(count_label);

        let tab_control = &ivars.tab_control;
        let titles: Vec<Retained<NSString>> = DocumentLensTab::ALL_CASES.iter().map(|tab| ns_string(tab.title())).collect();
        tab_control.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
        tab_control.setFont(Some(&PanelFont::row()));
        unsafe {
            tab_control.setTarget(Some(object(self)));
            tab_control.setAction(Some(sel!(tabChanged:)));
        }
        set_label(&**tab_control, "Contents and Outline sections");
        tab_control.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(tab_control);

        let target_control = &ivars.target_control;
        let names: Vec<Retained<NSString>> =
            RenderTargetProfile::built_ins().iter().map(|profile| ns_string(&profile.name)).collect();
        target_control.addItemsWithTitles(&NSArray::from_retained_slice(&names));
        unsafe {
            target_control.setTarget(Some(object(self)));
            target_control.setAction(Some(sel!(targetChanged:)));
        }
        target_control.setFont(Some(&PanelFont::secondary()));
        set_label(&**target_control, "Render target");
        target_control.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(target_control);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            count_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            count_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            count_label
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            tab_control.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            tab_control.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            tab_control.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 8.0),
            target_control.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            target_control.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            target_control.topAnchor().constraintEqualToAnchor_constant(&tab_control.bottomAnchor(), 4.0),
        ]);
        self.sync_tab_control();
        self.sync_target_control();
    }

    fn build_table(&self) {
        let ivars = self.ivars();
        let table = &ivars.table;
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        table.setRowHeight(PanelMetrics::DETAIL_ROW_HEIGHT);
        unsafe {
            table.setTarget(Some(object(self)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<DocumentLensView> = ObjcWeak::from(self);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&**table, "Document Lens items");
        let scroll = self.scroll();
        self.addSubview(&scroll);
        ivars.empty_state.install(self, &scroll, 1.0);
        activate(&[
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&ivars.target_control.bottomAnchor(), 6.0),
            scroll.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);
    }

    // MARK: Properties

    pub fn delegate(&self) -> Option<Rc<dyn DocumentLensViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn DocumentLensViewDelegate>>) {
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

    pub fn model(&self) -> Rc<DocumentLensModel> {
        self.ivars().model.borrow().clone()
    }

    pub fn set_model(&self, model: DocumentLensModel) {
        *self.ivars().model.borrow_mut() = Rc::new(model);
        self.reload();
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
        *ivars.line_index.borrow_mut() =
            if source_text.is_empty() { None } else { Some(Rc::new(SourceLineIndex::new(source_text))) };
        ivars.table.reloadData();
    }

    pub fn selected_tab(&self) -> DocumentLensTab {
        self.ivars().selected_tab.get()
    }

    pub fn set_selected_tab(&self, tab: DocumentLensTab) {
        let old_value = self.ivars().selected_tab.replace(tab);
        if tab == old_value {
            return;
        }
        self.sync_tab_control();
        self.reload();
    }

    pub fn render_target_profile(&self) -> RenderTargetProfile {
        self.ivars().render_target_profile.borrow().clone()
    }

    pub fn set_render_target_profile(&self, profile: RenderTargetProfile) {
        let old_value = self.ivars().render_target_profile.replace(profile.clone());
        if profile == old_value {
            return;
        }
        self.sync_target_control();
    }

    pub fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }

    pub fn reload(&self) {
        let ivars = self.ivars();
        let model = self.model();
        let selected_tab = ivars.selected_tab.get();
        let section = model.section(selected_tab);
        *ivars.rows.borrow_mut() = section
            .groups
            .iter()
            .flat_map(|group| {
                std::iter::once(Row::Group(group.clone())).chain(group.items.iter().cloned().map(Row::Item))
            })
            .collect();
        let count = section.count();
        ivars.count_label.setStringValue(&ns_string(&if count == 0 { "None".to_owned() } else { format!("{count}") }));
        set_label(&*ivars.count_label, &format!("{count} items"));
        for (index, tab) in DocumentLensTab::ALL_CASES.iter().enumerate() {
            let count = model.section(*tab).count();
            if let Some(item) = ivars.tab_control.itemAtIndex(index as isize) {
                let title = if count == 0 { tab.title().to_owned() } else { format!("{}  \u{00B7}  {count}", tab.title()) };
                item.setTitle(&ns_string(&title));
            }
        }
        self.sync_tab_control();
        ivars.table.reloadData();
        self.restore_selection();
        self.update_empty_state(&section);
    }

    /// Reselect the same item, wherever the reparse moved it to.
    fn restore_selection(&self) {
        let ivars = self.ivars();
        let row = (|| {
            let selected_item_id = ivars.selected_item_id.borrow().clone()?;
            ivars.rows.borrow().iter().position(|row| match row {
                Row::Item(item) => item.id == selected_item_id,
                Row::Group(_) => false,
            })
        })();
        let Some(row) = row else {
            unsafe { ivars.table.deselectAll(None) };
            return;
        };
        ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
    }

    fn update_empty_state(&self, _section: &DocumentLensSection) {
        let ivars = self.ivars();
        let scroll = self.scroll();
        if !ivars.rows.borrow().is_empty() {
            ivars.empty_state.setHidden(true);
            scroll.setHidden(false);
            return;
        }
        let tab = ivars.selected_tab.get();
        let style_sheet = self.style_sheet();
        ivars.empty_state.configure(
            Self::empty_symbol(tab),
            Self::empty_title(tab),
            Self::empty_subtitle(tab),
            &style_sheet,
        );
        ivars.empty_state.setHidden(false);
        scroll.setHidden(true);
    }

    fn empty_symbol(tab: DocumentLensTab) -> &'static str {
        match tab {
            DocumentLensTab::Structure => "list.bullet.indent",
            DocumentLensTab::Health => "checkmark.seal",
            DocumentLensTab::Links => "link",
            DocumentLensTab::Assets => "photo.on.rectangle",
            DocumentLensTab::Tasks => "checklist",
            DocumentLensTab::Changes => "checkmark.seal",
            DocumentLensTab::RenderTarget => "checkmark.seal",
        }
    }

    fn empty_title(tab: DocumentLensTab) -> &'static str {
        match tab {
            DocumentLensTab::Structure => "No structure yet",
            DocumentLensTab::Health => "No findings",
            DocumentLensTab::Links => "No links",
            DocumentLensTab::Assets => "No images",
            DocumentLensTab::Tasks => "No tasks",
            DocumentLensTab::Changes => "No changes",
            DocumentLensTab::RenderTarget => "Compatible",
        }
    }

    fn empty_subtitle(tab: DocumentLensTab) -> &'static str {
        match tab {
            DocumentLensTab::Structure => "Add a heading to give this\ndocument a shape.",
            DocumentLensTab::Health => "Nothing in this document looks\nwrong from here.",
            DocumentLensTab::Links => "This document links nowhere yet.",
            DocumentLensTab::Assets => "This document has no image\nreferences.",
            DocumentLensTab::Tasks => "Add `- [ ]` to start a worklist.",
            DocumentLensTab::Changes => "Nothing has changed since\nyou last looked.",
            DocumentLensTab::RenderTarget => "Everything here renders on\nthe selected target.",
        }
    }

    /// Test and accessibility harness entry point. Production keyboard
    /// activation uses the same `PanelTableView` path.
    pub fn select_item_for_testing(&self, row: isize) {
        let ivars = self.ivars();
        {
            let rows = ivars.rows.borrow();
            if !(row >= 0 && (row as usize) < rows.len() && matches!(rows[row as usize], Row::Item(_))) {
                return;
            }
        }
        ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        self.activate_selection();
    }

    fn sync_tab_control(&self) {
        let ivars = self.ivars();
        let selected_tab = ivars.selected_tab.get();
        let Some(index) = DocumentLensTab::ALL_CASES.iter().position(|tab| *tab == selected_tab) else { return };
        ivars.tab_control.selectItemAtIndex(index as isize);
        self.theme(&ivars.tab_control);
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.count_label.setTextColor(Some(&style_sheet.text_faint));
        self.theme(&ivars.tab_control);
        self.theme(&ivars.target_control);
        ivars.table.reloadData();
    }

    fn theme(&self, control: &NSPopUpButton) {
        let style_sheet = self.style_sheet();
        control.setContentTintColor(Some(&style_sheet.text));
        let Some(title) = control.selectedItem().map(|item| item.title().to_string()) else { return };
        let font = control.font().unwrap_or_else(PanelFont::row);
        let title = attributed_string(
            &title,
            &[(keys::foreground_color(), object(&*style_sheet.text)), (keys::font(), object(&*font))],
        );
        control.setAttributedTitle(&title);
    }

    fn target_changed(&self, sender: &NSPopUpButton) {
        let index = sender.indexOfSelectedItem();
        let built_ins = RenderTargetProfile::built_ins();
        if !(index >= 0 && (index as usize) < built_ins.len()) {
            return;
        }
        let profile = built_ins[index as usize].clone();
        self.set_render_target_profile(profile.clone());
        if let Some(delegate) = self.delegate() {
            delegate.document_lens_did_select_render_target(self, &profile);
        }
    }

    fn sync_target_control(&self) {
        let ivars = self.ivars();
        let profile = ivars.render_target_profile.borrow().clone();
        let Some(index) = RenderTargetProfile::built_ins().iter().position(|candidate| *candidate == profile) else {
            return;
        };
        ivars.target_control.selectItemAtIndex(index as isize);
        self.theme(&ivars.target_control);
    }

    fn activate_selection(&self) {
        let ivars = self.ivars();
        let clicked = ivars.table.clickedRow();
        let row = if clicked >= 0 { clicked } else { ivars.table.selectedRow() };
        let item = {
            let rows = ivars.rows.borrow();
            if !(row >= 0 && (row as usize) < rows.len()) {
                return;
            }
            let Row::Item(item) = &rows[row as usize] else { return };
            item.clone()
        };
        *ivars.selected_item_id.borrow_mut() = Some(item.id.clone());
        if let Some(delegate) = self.delegate() {
            delegate.document_lens_did_select(self, item.range, &item);
        }
    }

    // MARK: NSTableViewDataSource, NSTableViewDelegate

    fn height_of_row(&self, row: isize) -> CGFloat {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return PanelMetrics::DETAIL_ROW_HEIGHT;
        }
        if let Row::Group(_) = rows[row as usize] {
            return PanelMetrics::GROUP_ROW_HEIGHT;
        }
        PanelMetrics::DETAIL_ROW_HEIGHT
    }

    fn is_group_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        matches!(rows[row as usize], Row::Group(_))
    }

    fn should_select_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        !matches!(rows[row as usize], Row::Group(_))
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let ivars = self.ivars();
        let entry = {
            let rows = ivars.rows.borrow();
            if !(row < rows.len() as isize) {
                return None;
            }
            rows[row as usize].clone()
        };
        match entry {
            Row::Group(group) => {
                let id = NSString::from_str("documentLensGroup");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<PanelGroupRowView>(&view))
                    .unwrap_or_else(|| PanelGroupRowView::new(&id, self.mtm()));
                let text = format!("{}  \u{00B7}  {}", swift::uppercased(&group.title), group.count());
                cell.configure(&text, &self.style_sheet().text_faint);
                Some(Retained::into_super(cell))
            }
            Row::Item(item) => {
                let id = NSString::from_str("documentLensItem");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| super::appkit_support::downcast::<DocumentLensRowView>(&view))
                    .unwrap_or_else(|| DocumentLensRowView::new(&id, self.mtm()));
                let line_index = ivars.line_index.borrow().clone();
                let line_caption = line_index.map(|index| index.caption(item.range));
                cell.configure(&item, line_caption.as_deref(), &self.style_sheet());
                Some(Retained::into_super(cell))
            }
        }
    }
}

// MARK: - DocumentLensRowView

pub struct DocumentLensRowViewIvars {
    title_label: Retained<NSTextField>,
    detail_label: Retained<NSTextField>,
}

define_class!(
    /// `DocumentLensRowView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentLensRowView"]
    #[ivars = DocumentLensRowViewIvars]
    pub struct DocumentLensRowView;

    unsafe impl NSObjectProtocol for DocumentLensRowView {}
);

impl DocumentLensRowView {
    /// `init(identifier:)`.
    pub fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<DocumentLensRowView> {
        let this = Self::alloc(mtm)
            .set_ivars(DocumentLensRowViewIvars { title_label: label("", mtm), detail_label: label("", mtm) });
        let this: Retained<DocumentLensRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        let ivars = this.ivars();
        ivars.title_label.setFont(Some(&PanelFont::row_emphasised()));
        ivars.detail_label.setFont(Some(&PanelFont::secondary()));
        for label in [&ivars.title_label, &ivars.detail_label] {
            label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            label.setTranslatesAutoresizingMaskIntoConstraints(false);
            this.addSubview(label);
        }
        let (title, detail) = (&ivars.title_label, &ivars.detail_label);
        activate(&[
            title.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            title.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            title.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 7.0),
            detail.leadingAnchor().constraintEqualToAnchor(&title.leadingAnchor()),
            detail.trailingAnchor().constraintEqualToAnchor(&title.trailingAnchor()),
            detail.topAnchor().constraintEqualToAnchor_constant(&title.bottomAnchor(), 2.0),
        ]);
        set_role(&*this, role::button());
        this
    }

    pub fn configure(&self, item: &DocumentLensItem, line_caption: Option<&str>, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        ivars.title_label.setStringValue(&ns_string(&item.title));
        let detail: Vec<&str> =
            [line_caption, if item.detail.is_empty() { None } else { Some(item.detail.as_str()) }].into_iter().flatten().collect();
        ivars.detail_label.setStringValue(&ns_string(&detail.join("  \u{00B7}  ")));
        ivars.title_label.setTextColor(Some(&Self::color(item.severity, style_sheet)));
        ivars.detail_label.setTextColor(Some(&style_sheet.text_faint));
        let tool_tip =
            if item.detail.is_empty() { item.title.clone() } else { format!("{}\n{}", item.title, item.detail) };
        self.setToolTip(Some(&ns_string(&tool_tip)));
        let position = match line_caption {
            Some(caption) => caption.to_owned(),
            None => format!("Source range {}\u{2013}{}", item.range.location, item.range.location + item.range.length),
        };
        set_label(self, &format!("{}, {}, {position}", item.title, item.detail));
        set_role(self, role::button());
    }

    fn color(severity: Option<DocumentLensSeverity>, style_sheet: &StyleSheet) -> Retained<NSColor> {
        match severity {
            Some(DocumentLensSeverity::Error) => style_sheet.callout_color(CalloutKind::Danger),
            Some(DocumentLensSeverity::Warning) => style_sheet.callout_color(CalloutKind::Warning),
            Some(DocumentLensSeverity::Info) | None => style_sheet.text_secondary.clone(),
        }
    }
}
