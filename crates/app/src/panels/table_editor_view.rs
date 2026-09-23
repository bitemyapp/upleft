//! Port of `Panels/TableEditorView.swift`: a small, source-preserving table
//! editor.
//!
//! The view edits one cell or one structural operation at a time. The core
//! proposes the exact source replacement. The host owns the document
//! mutation, so every accepted operation has one undo step and no hidden
//! re-serialise.
//!
//! Objective-C class names: `TableEditorView`, `TableEditorCell` (Swift's
//! private `NSTextField` subclass).
//!
//! Reproduced as Swift behaves:
//! - `tableView(_:didClick:row:)` matches no `NSTableViewDelegate`
//!   requirement (the real one is `tableView(_:didClick:)`), so Swift never
//!   exposes it to Objective-C and AppKit never calls it. It is an inherent
//!   method here, equally unreachable from AppKit.
//! - `PanelList.makeTableView` removes the header view, so the column titles
//!   `rebuildColumns` sets are never drawn.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSCellImagePosition, NSColor, NSControl, NSControlSize, NSControlTextEditingDelegate,
    NSEvent, NSEventModifierFlags, NSLayoutAttribute, NSPopUpButton, NSResponder, NSScrollView, NSStackView,
    NSTableColumn, NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextAlignment,
    NSTextField, NSTextFieldDelegate, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSIndexSet, NSNotification, NSRect, NSString};
use upleft_core::NSRange;
use upleft_core::editing::table_editing::{TableEditOperation, TableEditProposal, TableEditing};
use upleft_core::model::{BlockContent, ParsedDocument, TableAlignment, TableData};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::appkit_support::{activate, is_kind_of, label, main_async, ns_string, role, set_label, set_role, system_symbol};
use super::panel_chrome::{
    PanelBackdrop, PanelFont, PanelList, PanelMetrics, PanelSurface, PanelTableView, element_at, install_backdrop,
};

/// `TableEditorDelegate`.
pub trait TableEditorDelegate {
    fn table_editor_did_apply(&self, editor: &TableEditorView, proposal: &TableEditProposal);
    fn table_editor_did_request_source(&self, editor: &TableEditorView, range: NSRange);
    /// The user is finished.  Every accepted operation was already written
    /// through `didApply`, so the host only has to take the sheet down.
    ///
    /// Dismissal must work before any host adopts the two new callbacks: a
    /// sheet with no exit is worse than a sheet whose host does not tidy up
    /// after it.
    fn table_editor_did_finish(&self, editor: &TableEditorView) {
        editor.dismiss_hosting_window();
    }
    /// The user backed out.  `editor.applied_edit_count()` is exactly how
    /// many undo steps this session wrote, so a host that wants a true
    /// revert can undo that many and nothing else.
    fn table_editor_did_cancel(&self, editor: &TableEditorView) {
        editor.dismiss_hosting_window();
    }
}

/// Named so `isEnabled` is driven by a value, not by a button title.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TableColumnOperation {
    Add = 0,
    Delete = 1,
    MoveLeft = 2,
    MoveRight = 3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableRowOperation {
    Add = 0,
    Delete = 1,
    MoveUp = 2,
    MoveDown = 3,
}

const ALIGNMENT_ORDER: [TableAlignment; 4] =
    [TableAlignment::None, TableAlignment::Left, TableAlignment::Center, TableAlignment::Right];

pub struct TableEditorViewIvars {
    delegate: RefCell<Option<Weak<dyn TableEditorDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
    table_view: Retained<PanelTableView>,
    /// `private lazy var scrollView`.
    scroll_view: OnceCell<Retained<NSScrollView>>,
    alignment_popup: Retained<NSPopUpButton>,
    source_button: Retained<NSButton>,
    done_button: Retained<NSButton>,
    cancel_button: Retained<NSButton>,
    /// Structural buttons, kept so selection can drive `isEnabled` rather
    /// than letting an invalid press write an explanation into the status
    /// label.
    column_buttons: RefCell<[Option<Retained<NSButton>>; 4]>,
    row_buttons: RefCell<[Option<Retained<NSButton>>; 4]>,
    document: RefCell<Arc<ParsedDocument>>,
    table_index: Cell<isize>,
    data: RefCell<Option<TableData>>,
    values: RefCell<Vec<Vec<String>>>,
    alignments: RefCell<Vec<TableAlignment>>,
    table_range: Cell<NSRange>,
    selected_column_value: Cell<isize>,
    /// Undo steps this session has written, so a cancelling host knows
    /// exactly how far to roll back.
    applied_edit_count: Cell<isize>,
}

define_class!(
    /// `TableEditorView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TableEditorView"]
    #[ivars = TableEditorViewIvars]
    pub struct TableEditorView;

    unsafe impl NSObjectProtocol for TableEditorView {}

    impl TableEditorView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::WIDE_WIDTH
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }

        #[unsafe(method(addRow:))]
        fn __add_row(&self, _sender: Option<&AnyObject>) {
            self.add_row();
        }

        #[unsafe(method(deleteRow:))]
        fn __delete_row(&self, _sender: Option<&AnyObject>) {
            self.delete_row();
        }

        #[unsafe(method(addColumn:))]
        fn __add_column(&self, _sender: Option<&AnyObject>) {
            self.add_column();
        }

        #[unsafe(method(deleteColumn:))]
        fn __delete_column(&self, _sender: Option<&AnyObject>) {
            self.delete_column();
        }

        #[unsafe(method(moveRowUp:))]
        fn __move_row_up(&self, _sender: Option<&AnyObject>) {
            self.move_row_up();
        }

        #[unsafe(method(moveRowDown:))]
        fn __move_row_down(&self, _sender: Option<&AnyObject>) {
            self.move_row_down();
        }

        #[unsafe(method(moveColumnLeft:))]
        fn __move_column_left(&self, _sender: Option<&AnyObject>) {
            self.move_column_left();
        }

        #[unsafe(method(moveColumnRight:))]
        fn __move_column_right(&self, _sender: Option<&AnyObject>) {
            self.move_column_right();
        }

        #[unsafe(method(alignmentChanged:))]
        fn __alignment_changed(&self, sender: &NSPopUpButton) {
            self.alignment_changed(sender);
        }

        #[unsafe(method(requestSource:))]
        fn __request_source(&self, _sender: Option<&AnyObject>) {
            self.request_source();
        }

        #[unsafe(method(finish:))]
        fn __finish(&self, _sender: Option<&AnyObject>) {
            self.finish();
        }

        #[unsafe(method(cancel:))]
        fn __cancel(&self, _sender: Option<&AnyObject>) {
            self.cancel();
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.cancel();
        }

        /// So does clicking a cell.  Requiring a header click was the whole
        /// reason the column operations looked broken: selecting a cell left
        /// the column at -1 and every column button silently refused.
        #[unsafe(method(cellClicked:))]
        fn __cell_clicked(&self, _sender: Option<&AnyObject>) {
            let clicked = self.ivars().table_view.clickedColumn();
            if clicked >= 0 {
                self.select_column(clicked);
            }
        }
    }

    unsafe impl NSTableViewDataSource for TableEditorView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().values.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for TableEditorView {
        #[unsafe(method(controlTextDidBeginEditing:))]
        fn __control_text_did_begin_editing(&self, notification: &NSNotification) {
            if let Some(field) = notification_field(notification) {
                self.select_column(field.tag() % 10_000);
            }
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn __control_text_did_end_editing(&self, notification: &NSNotification) {
            self.control_text_did_end_editing(notification);
        }
    }

    unsafe impl NSTextFieldDelegate for TableEditorView {}

    unsafe impl NSTableViewDelegate for TableEditorView {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, table_column, row)
        }

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn __row_view_for_row(&self, table_view: &NSTableView, _row: isize) -> Option<Retained<NSTableRowView>> {
            let style_sheet = self.style_sheet();
            let owner: &AnyObject = self.as_ref();
            Some(Retained::into_super(PanelList::selection_row(table_view, Some(owner), style_sheet, self.mtm())))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn __selection_did_change(&self, _notification: &NSNotification) {
            self.update_alignment_selection();
            self.update_operation_availability();
        }
    }
);

/// `notification.object as? NSTextField`.
fn notification_field(notification: &NSNotification) -> Option<Retained<NSTextField>> {
    let object = notification.object()?;
    super::appkit_support::downcast::<NSTextField>(&object)
}

impl PanelSurface for TableEditorView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::WIDE_WIDTH
    }
}

impl TableEditorView {
    /// `init(document:tableIndex:styleSheet:)`; Swift's defaults are
    /// `tableIndex: 0` and `StyleSheet.current` (see [`Self::new_default`]).
    pub fn new(
        document: Arc<ParsedDocument>,
        table_index: isize,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<TableEditorView> {
        // Stored-property initial values, in declaration order.
        let title_label = label("Table", mtm);
        let status_label = label("", mtm);
        let table_view = {
            let table = PanelList::make_table_view("tableEditorSeed", mtm);
            // Swift bridges `tableColumns` to an Array (a copy) first.
            for column in table.tableColumns().to_vec() {
                table.removeTableColumn(&column);
            }
            table
        };
        let alignment_popup = NSPopUpButton::new(mtm);
        let source_button = unsafe { NSButton::buttonWithTitle_target_action(&ns_string("Edit Source"), None, None, mtm) };
        let done_button = unsafe { NSButton::buttonWithTitle_target_action(&ns_string("Done"), None, None, mtm) };
        let cancel_button = unsafe { NSButton::buttonWithTitle_target_action(&ns_string("Cancel"), None, None, mtm) };
        // The init body.
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(TableEditorViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            backdrop,
            title_label,
            status_label,
            table_view,
            scroll_view: OnceCell::new(),
            alignment_popup,
            source_button,
            done_button,
            cancel_button,
            column_buttons: RefCell::new([None, None, None, None]),
            row_buttons: RefCell::new([None, None, None, None]),
            document: RefCell::new(document),
            table_index: Cell::new(table_index),
            data: RefCell::new(None),
            values: RefCell::new(Vec::new()),
            alignments: RefCell::new(Vec::new()),
            table_range: Cell::new(NSRange::new(0, 0)),
            selected_column_value: Cell::new(-1),
            applied_edit_count: Cell::new(0),
        });
        let this: Retained<TableEditorView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.build_interface();
        this.reload();
        this
    }

    /// `TableEditorView(document:)` with the Swift defaults.
    pub fn new_default(document: Arc<ParsedDocument>, mtm: MainThreadMarker) -> Retained<TableEditorView> {
        Self::new(document, 0, Rc::new(StyleSheet::current(mtm)), mtm)
    }

    pub fn delegate(&self) -> Option<Rc<dyn TableEditorDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn TableEditorDelegate>>) {
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

    pub fn document(&self) -> Arc<ParsedDocument> {
        self.ivars().document.borrow().clone()
    }

    pub fn table_index(&self) -> isize {
        self.ivars().table_index.get()
    }

    pub fn applied_edit_count(&self) -> isize {
        self.ivars().applied_edit_count.get()
    }

    pub fn row_count_for_testing(&self) -> isize {
        self.ivars().values.borrow().len() as isize
    }

    pub fn column_count_for_testing(&self) -> isize {
        self.ivars().data.borrow().as_ref().map_or(0, TableData::column_count)
    }

    pub fn source_range_for_testing(&self) -> NSRange {
        self.ivars().table_range.get()
    }

    fn scroll_view(&self) -> Retained<NSScrollView> {
        self.ivars()
            .scroll_view
            .get_or_init(|| PanelList::make_scroll_view(&self.ivars().table_view, self.mtm()))
            .clone()
    }

    /// Replace the parsed snapshot after the host accepts a proposal.
    pub fn update(&self, document: Arc<ParsedDocument>) {
        *self.ivars().document.borrow_mut() = document;
        self.reload();
    }

    /// Select a cell without changing source. Useful for keyboard and tests.
    pub fn select(&self, row: isize, column: isize) {
        let table_view = self.ivars().table_view.clone();
        if !(row >= 0 && row < table_view.numberOfRows() && column >= 0 && column < table_view.numberOfColumns()) {
            return;
        }
        table_view.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        self.ivars().selected_column_value.set(column);
        table_view.scrollRowToVisible(row);
        self.update_alignment_selection();
        self.update_operation_availability();
    }

    pub fn apply(&self, operation: &TableEditOperation) {
        self.propose(operation);
    }

    pub fn request_source_for_testing(&self) {
        self.request_source();
    }

    fn build_interface(&self) {
        let ivars = self.ivars();
        install_backdrop(self, &ivars.backdrop);

        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::title()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);

        let status_label = &ivars.status_label;
        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        status_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(status_label);

        let table_view = &ivars.table_view;
        unsafe {
            table_view.setDelegate(Some(ProtocolObject::from_ref(self)));
            table_view.setDataSource(Some(ProtocolObject::from_ref(self)));
        }
        table_view.setUsesAlternatingRowBackgroundColors(false);
        table_view.setRowHeight(PanelMetrics::LIST_ROW_HEIGHT);
        table_view.setAllowsColumnReordering(false);
        table_view.setAllowsEmptySelection(true);
        unsafe {
            table_view.setTarget(Some(self.as_ref()));
            table_view.setAction(Some(sel!(cellClicked:)));
        }
        set_label(&**table_view, "Editable markdown table");
        let scroll_view = self.scroll_view();
        scroll_view.setHasHorizontalScroller(true);
        self.addSubview(&scroll_view);

        self.configure_alignment_popup();
        let controls = self.make_controls();
        self.addSubview(&controls);

        let source_button = &ivars.source_button;
        source_button.setBezelStyle(NSBezelStyle::Push);
        source_button.setControlSize(NSControlSize::Small);
        set_label(&**source_button, "Edit table source");
        unsafe {
            source_button.setTarget(Some(self.as_ref()));
            source_button.setAction(Some(sel!(requestSource:)));
        }
        source_button.setHidden(true);
        source_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(source_button);
        activate(&[
            source_button
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            source_button.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
        ]);

        // Done and Cancel are unconditional.  Every other control in this
        // sheet can be absent — there may be no table under the caret at all
        // — so the way out must not be one of the things that can disappear.
        let dismiss = self.make_dismiss_bar();
        self.addSubview(&dismiss);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 10.0),
            status_label.leadingAnchor().constraintEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            status_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            status_label
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&source_button.leadingAnchor(), -8.0),
            scroll_view.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            scroll_view.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            scroll_view.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 8.0),
            scroll_view.bottomAnchor().constraintEqualToAnchor_constant(&controls.topAnchor(), -8.0),
            controls.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            controls
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            controls.bottomAnchor().constraintEqualToAnchor_constant(&dismiss.topAnchor(), -10.0),
            controls.heightAnchor().constraintEqualToConstant(30.0),
            dismiss.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            dismiss
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            dismiss.bottomAnchor().constraintEqualToAnchor_constant(&self.bottomAnchor(), -PanelMetrics::INSET),
        ]);

        set_role(self, role::group());
        set_label(self, "Table editor");
        self.apply_style();
        self.update_operation_availability();
    }

    fn make_dismiss_bar(&self) -> Retained<NSStackView> {
        let ivars = self.ivars();
        let cancel_button = &ivars.cancel_button;
        cancel_button.setBezelStyle(NSBezelStyle::Push);
        unsafe {
            cancel_button.setTarget(Some(self.as_ref()));
            cancel_button.setAction(Some(sel!(cancel:)));
        }
        cancel_button.setKeyEquivalent(&ns_string("\u{1b}"));
        set_label(&**cancel_button, "Close the table editor");
        cancel_button.setTranslatesAutoresizingMaskIntoConstraints(false);

        let done_button = &ivars.done_button;
        done_button.setBezelStyle(NSBezelStyle::Push);
        unsafe {
            done_button.setTarget(Some(self.as_ref()));
            done_button.setAction(Some(sel!(finish:)));
        }
        done_button.setKeyEquivalent(&ns_string("\r"));
        set_label(&**done_button, "Finish editing the table");
        done_button.setTranslatesAutoresizingMaskIntoConstraints(false);

        let views: Retained<NSArray<NSView>> = NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(cancel_button.clone())),
            Retained::into_super(Retained::into_super(done_button.clone())),
        ]);
        let stack = NSStackView::stackViewWithViews(&views, self.mtm());
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        stack.setAlignment(NSLayoutAttribute::CenterY);
        stack.setSpacing(8.0);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack
    }

    fn configure_alignment_popup(&self) {
        let popup = &self.ivars().alignment_popup;
        popup.removeAllItems();
        let titles: Retained<NSArray<NSString>> =
            NSArray::from_retained_slice(&["Automatic", "Left", "Center", "Right"].map(ns_string));
        popup.addItemsWithTitles(&titles);
        unsafe {
            popup.setTarget(Some(self.as_ref()));
            popup.setAction(Some(sel!(alignmentChanged:)));
        }
        set_label(&**popup, "Column alignment");
        popup.setTranslatesAutoresizingMaskIntoConstraints(false);
    }

    fn make_controls(&self) -> Retained<NSStackView> {
        let stack = NSStackView::new(self.mtm());
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        stack.setSpacing(5.0);
        stack.setAlignment(NSLayoutAttribute::CenterY);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);

        // Row and column glyphs differ by badge, not by tooltip: a
        // plus-badged rectangle and a minus-badged rectangle cannot be
        // confused the way `split.3x1` and `split.2x1` can.
        let row = |operation: TableRowOperation, button: Retained<NSButton>| {
            self.ivars().row_buttons.borrow_mut()[operation as usize] = Some(button);
        };
        let column = |operation: TableColumnOperation, button: Retained<NSButton>| {
            self.ivars().column_buttons.borrow_mut()[operation as usize] = Some(button);
        };
        row(TableRowOperation::Add, self.add_button("Add Row", "plus", sel!(addRow:), &stack));
        row(TableRowOperation::Delete, self.add_button("Delete Row", "minus", sel!(deleteRow:), &stack));
        column(TableColumnOperation::Add, self.add_button("Add Column", "rectangle.badge.plus", sel!(addColumn:), &stack));
        column(
            TableColumnOperation::Delete,
            self.add_button("Delete Column", "rectangle.badge.minus", sel!(deleteColumn:), &stack),
        );
        row(TableRowOperation::MoveUp, self.add_button("Move Row Up", "arrow.up", sel!(moveRowUp:), &stack));
        row(TableRowOperation::MoveDown, self.add_button("Move Row Down", "arrow.down", sel!(moveRowDown:), &stack));
        column(
            TableColumnOperation::MoveLeft,
            self.add_button("Move Column Left", "arrow.left", sel!(moveColumnLeft:), &stack),
        );
        column(
            TableColumnOperation::MoveRight,
            self.add_button("Move Column Right", "arrow.right", sel!(moveColumnRight:), &stack),
        );
        stack.addArrangedSubview(&self.ivars().alignment_popup);
        stack
    }

    fn add_button(&self, title: &str, symbol: &str, selector: Sel, stack: &NSStackView) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(&ns_string(title), Some(self.as_ref()), Some(selector), self.mtm())
        };
        if let Some(image) = system_symbol(symbol, Some(title)) {
            button.setImage(Some(&image));
            button.setImagePosition(NSCellImagePosition::ImageOnly);
        }
        button.setToolTip(Some(&ns_string(title)));
        set_label(&*button, title);
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.addArrangedSubview(&button);
        button
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        ivars.status_label.setTextColor(Some(&style_sheet.text_secondary));
        self.setNeedsDisplay(true);
    }

    // MARK: - Source model

    fn table_block(&self) -> Option<(NSRange, TableData)> {
        let document = self.document();
        let mut matches: Vec<(NSRange, TableData)> = Vec::new();
        document.root.walk(&mut |block| {
            if let BlockContent::Table(data) = &block.content {
                matches.push((block.range, data.clone()));
            }
        });
        let table_index = self.ivars().table_index.get();
        if !(table_index >= 0 && (table_index as usize) < matches.len()) {
            return None;
        }
        Some(matches.swap_remove(table_index as usize))
    }

    pub fn reload(&self) {
        let ivars = self.ivars();
        let table_view = ivars.table_view.clone();
        let Some((range, table)) = self.table_block() else {
            *ivars.data.borrow_mut() = None;
            ivars.values.borrow_mut().clear();
            ivars.alignments.borrow_mut().clear();
            ivars.selected_column_value.set(-1);
            ivars.table_range.set(NSRange::new(0, 0));
            ivars.title_label.setStringValue(&ns_string("Table"));
            ivars.status_label.setStringValue(&ns_string("No table under the caret"));
            ivars.source_button.setHidden(true);
            for column in table_view.tableColumns().to_vec() {
                table_view.removeTableColumn(&column);
            }
            table_view.reloadData();
            self.update_operation_availability();
            return;
        };
        let document = self.document();
        let column_count = table.column_count();
        let row_count = table.rows.len() as isize;
        let values: Vec<Vec<String>> = table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| swift_text::trim_whitespaces_and_newlines(&document.substring(cell.content_range)).to_owned())
                    .collect()
            })
            .collect();
        *ivars.alignments.borrow_mut() = table.alignments.clone();
        *ivars.data.borrow_mut() = Some(table);
        ivars.table_range.set(range);
        *ivars.values.borrow_mut() = values;
        if ivars.selected_column_value.get() >= column_count {
            ivars.selected_column_value.set(column_count - 1);
        }
        ivars.title_label.setStringValue(&ns_string(&format!("Table {}", ivars.table_index.get() + 1)));
        ivars
            .status_label
            .setStringValue(&ns_string(&format!("{} columns · {} rows", column_count, 0.max(row_count - 1))));
        ivars.source_button.setHidden(false);
        self.rebuild_columns(column_count);
        table_view.reloadData();
        self.update_alignment_selection();
        self.update_operation_availability();
    }

    /// Columns are titled from the table's own header row.  A bare "1" gives
    /// a reader nothing to identify a column by, which is exactly what makes
    /// "select a column" feel like a guess.
    fn rebuild_columns(&self, count: isize) {
        let table_view = self.ivars().table_view.clone();
        for column in table_view.tableColumns().to_vec() {
            table_view.removeTableColumn(&column);
        }
        let headers: Vec<String> = self.ivars().values.borrow().first().cloned().unwrap_or_default();
        for index in 0..count {
            let column = NSTableColumn::initWithIdentifier(
                NSTableColumn::alloc(self.mtm()),
                &ns_string(&format!("column-{index}")),
            );
            let header =
                element_at(&headers, index).map(|header| swift_text::trim_whitespaces(header).to_owned()).unwrap_or_default();
            let title = if header.is_empty() { format!("Column {}", index + 1) } else { header };
            column.setTitle(&ns_string(&title));
            column.setMinWidth(90.0);
            column.setWidth(130.0);
            column.headerCell().setAlignment(NSTextAlignment::Center);
            table_view.addTableColumn(&column);
        }
    }

    fn selected_row(&self) -> isize {
        self.ivars().table_view.selectedRow()
    }

    fn selected_column(&self) -> isize {
        self.ivars().selected_column_value.get()
    }

    /// Structural operations are enabled only where they mean something, so
    /// an invalid press cannot happen and no status line has to explain one.
    fn update_operation_availability(&self) {
        let ivars = self.ivars();
        let (columns, rows, has_table) = {
            let data = ivars.data.borrow();
            (
                data.as_ref().map_or(0, TableData::column_count),
                data.as_ref().map_or(0, |data| data.rows.len() as isize),
                data.is_some(),
            )
        };
        let column = self.selected_column();
        let row = self.selected_row();

        let row_buttons = ivars.row_buttons.borrow().clone();
        let column_buttons = ivars.column_buttons.borrow().clone();
        let set = |button: &Option<Retained<NSButton>>, enabled: bool| {
            if let Some(button) = button {
                button.setEnabled(enabled);
            }
        };
        set(&row_buttons[TableRowOperation::Add as usize], has_table);
        set(&row_buttons[TableRowOperation::Delete as usize], row > 0);
        set(&row_buttons[TableRowOperation::MoveUp as usize], row > 1);
        set(&row_buttons[TableRowOperation::MoveDown as usize], row > 0 && row + 1 < rows);
        set(&column_buttons[TableColumnOperation::Add as usize], has_table);
        set(&column_buttons[TableColumnOperation::Delete as usize], column >= 0 && columns > 1);
        set(&column_buttons[TableColumnOperation::MoveLeft as usize], column > 0);
        set(&column_buttons[TableColumnOperation::MoveRight as usize], column >= 0 && column + 1 < columns);
        ivars.alignment_popup.setEnabled(column >= 0);
        ivars.source_button.setEnabled(ivars.table_range.get().length > 0);
    }

    fn propose(&self, operation: &TableEditOperation) {
        let document = self.document();
        let result = TableEditing::propose(&document, self.ivars().table_index.get(), operation);
        let Some(proposal) = result.proposal else {
            self.ivars().status_label.setStringValue(&ns_string("Source edit is not available"));
            return;
        };
        self.ivars().applied_edit_count.set(self.ivars().applied_edit_count.get() + 1);
        if let Some(delegate) = self.delegate() {
            delegate.table_editor_did_apply(self, &proposal);
        }
    }

    // MARK: - Controls

    /// `data.rows.count` and `data.columnCount`, when there is a table.
    fn data_counts(&self) -> Option<(isize, isize)> {
        self.ivars().data.borrow().as_ref().map(|data| (data.rows.len() as isize, data.column_count()))
    }

    fn add_row(&self) {
        let Some((rows, columns)) = self.data_counts() else { return };
        let index = if self.selected_row() >= 1 { (self.selected_row() + 1).min(rows) } else { rows };
        self.propose(&TableEditOperation::InsertRow { index, cells: vec![String::new(); columns as usize] });
    }

    fn delete_row(&self) {
        if !(self.selected_row() > 0) {
            return;
        }
        self.propose(&TableEditOperation::DeleteRow { index: self.selected_row() });
    }

    fn add_column(&self) {
        let Some((_, columns)) = self.data_counts() else { return };
        let index = if self.selected_column() >= 0 { self.selected_column() + 1 } else { columns };
        self.propose(&TableEditOperation::InsertColumn { index, header: String::new(), cells: Vec::new() });
    }

    fn delete_column(&self) {
        if !(self.selected_column() >= 0) {
            return;
        }
        self.propose(&TableEditOperation::DeleteColumn { index: self.selected_column() });
    }

    fn move_row_up(&self) {
        if !(self.selected_row() > 1) {
            return;
        }
        self.propose(&TableEditOperation::MoveRow { from: self.selected_row(), to: self.selected_row() - 1 });
    }

    fn move_row_down(&self) {
        let Some((rows, _)) = self.data_counts() else { return };
        if !(self.selected_row() > 0 && self.selected_row() + 1 < rows) {
            return;
        }
        self.propose(&TableEditOperation::MoveRow { from: self.selected_row(), to: self.selected_row() + 1 });
    }

    fn move_column_left(&self) {
        if !(self.selected_column() > 0) {
            return;
        }
        self.propose(&TableEditOperation::MoveColumn { from: self.selected_column(), to: self.selected_column() - 1 });
    }

    fn move_column_right(&self) {
        let Some((_, columns)) = self.data_counts() else { return };
        if !(self.selected_column() >= 0 && self.selected_column() + 1 < columns) {
            return;
        }
        self.propose(&TableEditOperation::MoveColumn { from: self.selected_column(), to: self.selected_column() + 1 });
    }

    fn alignment_changed(&self, sender: &NSPopUpButton) {
        let Some(alignment) = element_at(&ALIGNMENT_ORDER, sender.indexOfSelectedItem()).copied() else { return };
        if !(self.selected_column() >= 0) {
            return;
        }
        self.propose(&TableEditOperation::SetAlignment { column: self.selected_column(), alignment });
    }

    fn request_source(&self) {
        let range = self.ivars().table_range.get();
        if !(range.length > 0) {
            return;
        }
        if let Some(delegate) = self.delegate() {
            delegate.table_editor_did_request_source(self, range);
        }
    }

    // MARK: - Dismissal

    fn finish(&self) {
        self.commit_editing_cell();
        if let Some(delegate) = self.delegate() {
            delegate.table_editor_did_finish(self);
        } else {
            self.dismiss_hosting_window();
        }
    }

    fn cancel(&self) {
        if let Some(window) = self.window() {
            window.makeFirstResponder(None);
        }
        if let Some(delegate) = self.delegate() {
            delegate.table_editor_did_cancel(self);
        } else {
            self.dismiss_hosting_window();
        }
    }

    /// The AppKit-correct way out of whatever window is hosting the editor,
    /// used when no host has claimed the dismissal callbacks.
    pub fn dismiss_hosting_window(&self) {
        let Some(window) = self.window() else { return };
        if let Some(parent) = window.sheetParent() {
            parent.endSheet(&window);
        } else {
            window.close();
        }
    }

    /// Ends the field editor so a half-typed cell is proposed before the
    /// sheet goes away rather than lost with it.
    fn commit_editing_cell(&self) {
        let Some(window) = self.window() else { return };
        let is_text = window.firstResponder().is_some_and(|responder| is_kind_of(&responder, c"NSText"));
        if !is_text {
            return;
        }
        let table_view: &NSResponder = &self.ivars().table_view;
        window.makeFirstResponder(Some(table_view));
    }

    fn update_alignment_selection(&self) {
        let selected = self.selected_column();
        let alignment = {
            let alignments = self.ivars().alignments.borrow();
            if !(selected >= 0 && (selected as usize) < alignments.len()) {
                return;
            }
            alignments[selected as usize]
        };
        let index = ALIGNMENT_ORDER.iter().position(|candidate| *candidate == alignment).unwrap_or(0);
        self.ivars().alignment_popup.selectItemAtIndex(index as isize);
    }

    /// `window?.makeFirstResponder(view)`.
    fn make_first_responder(&self, view: &NSView) {
        if let Some(window) = self.window() {
            let responder: &NSResponder = view;
            window.makeFirstResponder(Some(responder));
        }
    }

    fn advance(&self, field: &TableEditorCell, forward: bool) {
        let row = field.tag() / 10_000;
        let column = field.tag() % 10_000;
        let values_count = self.ivars().values.borrow().len() as isize;
        if values_count == 0 {
            return;
        }
        let Some((_, column_count)) = self.data_counts() else { return };
        let next_column = column + if forward { 1 } else { -1 };
        let next_row;
        let target_column;
        if next_column >= column_count {
            if row + 1 >= values_count {
                self.add_row();
                let weak: ObjcWeak<TableEditorView> = ObjcWeak::from(self);
                main_async(move || {
                    let Some(this) = weak.load() else { return };
                    this.select(this.row_count_for_testing() - 1, 0);
                    let next = this.ivars().table_view.viewAtColumn_row_makeIfNecessary(
                        0,
                        this.row_count_for_testing() - 1,
                        true,
                    );
                    if let Some(next) = next {
                        this.make_first_responder(&next);
                    }
                });
                return;
            } else {
                next_row = row + 1;
                target_column = 0;
            }
        } else if next_column < 0 {
            next_row = 0.max(row - 1);
            target_column = 0.max(column_count - 1);
        } else {
            next_row = row;
            target_column = next_column;
        }
        self.select(next_row, target_column);
        if let Some(next) = self.ivars().table_view.viewAtColumn_row_makeIfNecessary(target_column, next_row, true) {
            self.make_first_responder(&next);
        }
    }

    fn advance_down(&self, field: &TableEditorCell) {
        let row = field.tag() / 10_000;
        let column = field.tag() % 10_000;
        let values_count = self.ivars().values.borrow().len() as isize;
        if values_count == 0 || self.ivars().data.borrow().is_none() {
            return;
        }
        if row + 1 >= values_count {
            self.add_row();
            let weak: ObjcWeak<TableEditorView> = ObjcWeak::from(self);
            main_async(move || {
                let Some(this) = weak.load() else { return };
                this.select(this.row_count_for_testing() - 1, column);
                let next =
                    this.ivars().table_view.viewAtColumn_row_makeIfNecessary(column, this.row_count_for_testing() - 1, true);
                if let Some(next) = next {
                    this.make_first_responder(&next);
                }
            });
        } else {
            self.select(row + 1, column);
            if let Some(next) = self.ivars().table_view.viewAtColumn_row_makeIfNecessary(column, row + 1, true) {
                self.make_first_responder(&next);
            }
        }
    }

    // MARK: - Table data

    fn view_for(
        &self,
        table_view: &NSTableView,
        table_column: Option<&NSTableColumn>,
        row: isize,
    ) -> Option<Retained<NSView>> {
        let table_column = table_column?;
        let column =
            table_view.tableColumns().iter().position(|candidate| candidate.isEqual(Some(table_column)))? as isize;
        let text = {
            let values = self.ivars().values.borrow();
            if !(row < values.len() as isize && column < values[row as usize].len() as isize) {
                return None;
            }
            values[row as usize][column as usize].clone()
        };
        let field = TableEditorCell::with_string(&text, self.mtm());
        let font = if row == 0 { PanelFont::row_emphasised() } else { PanelFont::row() };
        field.setFont(Some(&font));
        field.setBordered(false);
        field.setDrawsBackground(false);
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
        field.setTag(row * 10_000 + column);
        let weak_self: ObjcWeak<TableEditorView> = ObjcWeak::from(self);
        let weak_field: ObjcWeak<TableEditorCell> = ObjcWeak::from(&*field);
        *field.ivars().on_advance.borrow_mut() = Some(Rc::new(move |forward| {
            let (Some(this), Some(field)) = (weak_self.load(), weak_field.load()) else { return };
            this.advance(&field, forward);
        }));
        let weak_self: ObjcWeak<TableEditorView> = ObjcWeak::from(self);
        let weak_field: ObjcWeak<TableEditorCell> = ObjcWeak::from(&*field);
        *field.ivars().on_advance_down.borrow_mut() = Some(Rc::new(move || {
            let (Some(this), Some(field)) = (weak_self.load(), weak_field.load()) else { return };
            this.advance_down(&field);
        }));
        set_label(&*field, &format!("Table row {}, column {}", row + 1, column + 1));
        set_role(&*field, role::text_field());
        Some(Retained::into_super(Retained::into_super(Retained::into_super(field))))
    }

    /// `tableView(_:didClick:row:)`: "A header click selects a column."
    /// Never called by AppKit (see the module note); kept for fidelity.
    #[allow(dead_code)]
    fn table_view_did_click(&self, table_view: &NSTableView, table_column: Option<&NSTableColumn>, _row: isize) {
        let Some(table_column) = table_column else { return };
        let index = table_view
            .tableColumns()
            .iter()
            .position(|candidate| candidate.isEqual(Some(table_column)))
            .map_or(-1, |index| index as isize);
        self.select_column(index);
    }

    fn select_column(&self, column: isize) {
        self.ivars().selected_column_value.set(column);
        self.update_alignment_selection();
        self.update_operation_availability();
    }

    fn control_text_did_end_editing(&self, notification: &NSNotification) {
        let Some(field) = notification_field(notification) else { return };
        let row = field.tag() / 10_000;
        let column = field.tag() % 10_000;
        let string_value = field.stringValue().to_string();
        {
            let values = self.ivars().values.borrow();
            if !(row >= 0 && column >= 0 && row < values.len() as isize && column < values[row as usize].len() as isize) {
                return;
            }
            // Tabbing through a table must not write an undo step per cell:
            // only a value that actually changed is a change.
            if swift_text::str_eq(&values[row as usize][column as usize], &string_value) {
                return;
            }
        }
        self.propose(&TableEditOperation::SetCell { row, column, text: string_value });
    }
}

// MARK: - TableEditorCell

type AdvanceHandler = Rc<dyn Fn(bool)>;

pub struct TableEditorCellIvars {
    on_advance: RefCell<Option<AdvanceHandler>>,
    on_advance_down: RefCell<Option<Rc<dyn Fn()>>>,
}

define_class!(
    /// Swift's private `TableEditorCell`: Tab and Return move between cells.
    // SAFETY: `initWithFrame:` sets the ivars, so NSTextField's own class
    // factory (`+textFieldWithString:`) creates valid instances.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TableEditorCell"]
    #[ivars = TableEditorCellIvars]
    pub struct TableEditorCell;

    unsafe impl NSObjectProtocol for TableEditorCell {}

    impl TableEditorCell {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let this = this
                .set_ivars(TableEditorCellIvars { on_advance: RefCell::new(None), on_advance_down: RefCell::new(None) });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            let is_tab = event.keyCode() == 48;
            let is_return = event.keyCode() == 36 || event.keyCode() == 76;
            if is_tab {
                let handler = self.ivars().on_advance.borrow().clone();
                if let Some(handler) = handler {
                    handler(!event.modifierFlags().contains(NSEventModifierFlags::Shift));
                }
                return;
            } else if is_return {
                let handler = self.ivars().on_advance_down.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }
    }
);

impl TableEditorCell {
    /// `TableEditorCell(string:)`: NSTextField's class factory, sent to the
    /// subclass.
    pub fn with_string(text: &str, mtm: MainThreadMarker) -> Retained<TableEditorCell> {
        let _ = mtm;
        unsafe { msg_send![TableEditorCell::class(), textFieldWithString: &*ns_string(text)] }
    }
}
