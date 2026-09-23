//! Port of `Panels/CommandPaletteView.swift`: a compact, keyboard-first
//! command launcher. It owns only search state; command execution stays
//! with the window's single `perform(_:)` owner.
//!
//! Reproduced as Swift has it: the row highlights index the title's
//! attributed string with `FuzzyMatcher`'s *character* positions as UTF-16
//! offsets, so a title with astral or combined characters before a match
//! tints the wrong unit.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSColor, NSControlSize, NSControlTextEditingDelegate, NSEvent, NSEventMask,
    NSFocusRingType, NSLayoutAttribute, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLineBreakMode,
    NSResponder, NSScrollView, NSSearchField, NSSearchFieldDelegate, NSStackView, NSTableCellView, NSTableColumn,
    NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTableViewSelectionHighlightStyle, NSTextAlignment,
    NSTextField, NSTextFieldDelegate, NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSBorderType,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{
    NSIndexSet, NSMutableAttributedString, NSNotification, NSRange, NSRect, NSSize, NSString,
};
use upleft_render::appkit_compat::{attributes_dictionary, keys};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::appkit_support::{
    activate, cg, downcast, label, ns_string, object, role, set_help, set_label, set_role, set_value, weight_medium,
};
use super::fuzzy_matcher::FuzzyMatcher;
use super::panel_chrome::{ButtonAction, PanelBackdrop, PanelEmptyStateView, PanelFont, PanelMetrics, PanelSurface};
use crate::app::toolbar_controls::ToolbarInteractiveButton;
use crate::support::command_palette_model::{
    CommandPaletteModel, CommandPaletteRecentStore, UserDefaultsCommandPaletteRecentStore,
};
use crate::support::commands::Command;
use crate::support::quick_open_providers::{QuickOpenAction, QuickOpenProvider, QuickOpenResult};

/// `CommandPaletteViewDelegate`.
pub trait CommandPaletteViewDelegate {
    fn command_palette_did_choose(&self, palette: &CommandPaletteView, result: &QuickOpenResult);
    fn command_palette_did_cancel(&self, palette: &CommandPaletteView);
}

pub struct CommandPaletteViewIvars {
    delegate: RefCell<Option<Weak<dyn CommandPaletteViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    backdrop: Retained<PanelBackdrop>,
    search_field: Retained<NSSearchField>,
    table_view: Retained<NSTableView>,
    scroll_view: Retained<NSScrollView>,
    hint_label: Retained<NSTextField>,
    /// The prefixes exist and nothing said so. A filter you cannot discover
    /// is a filter nobody uses (§7.2).
    prefix_label: Retained<NSTextField>,
    empty_state: Retained<PanelEmptyStateView>,
    model: RefCell<CommandPaletteModel>,
    recent_store: Rc<dyn CommandPaletteRecentStore>,
    key_monitor: RefCell<Option<Retained<AnyObject>>>,
    actions: RefCell<Vec<Retained<ButtonAction>>>,
}

impl Drop for CommandPaletteViewIvars {
    fn drop(&mut self) {
        if let Some(key_monitor) = self.key_monitor.get_mut().take() {
            // SAFETY: the monitor came from `addLocalMonitorForEvents`.
            unsafe { NSEvent::removeMonitor(&key_monitor) };
        }
    }
}

define_class!(
    /// `CommandPaletteView`, a `PanelSurface`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "CommandPaletteView"]
    #[ivars = CommandPaletteViewIvars]
    pub struct CommandPaletteView;

    unsafe impl NSObjectProtocol for CommandPaletteView {}

    impl CommandPaletteView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::WIDE_WIDTH
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if self.window().is_none() {
                self.remove_key_monitor();
            } else if self.ivars().key_monitor.borrow().is_none() {
                self.install_key_monitor();
                if let Some(window) = self.window() {
                    window.makeFirstResponder(Some(&self.ivars().search_field));
                }
            }
        }

        /// Esc closes the palette even if the local key monitor is not
        /// installed — the responder chain answers as well as the monitor
        /// does.
        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            if let Some(delegate) = self.delegate() {
                delegate.command_palette_did_cancel(self);
            }
        }

        #[unsafe(method(doubleClick:))]
        fn __double_click(&self, sender: &NSTableView) {
            if !(sender.selectedRow() >= 0) {
                return;
            }
            self.ivars().model.borrow_mut().select(sender.selectedRow());
            self.choose_selection();
        }
    }

    unsafe impl NSControlTextEditingDelegate for CommandPaletteView {
        #[unsafe(method(controlTextDidChange:))]
        fn __control_text_did_change(&self, _notification: &NSNotification) {
            let query = self.ivars().search_field.stringValue().to_string();
            self.ivars().model.borrow_mut().update_query(&query);
            self.reload_results();
        }
    }

    unsafe impl NSTextFieldDelegate for CommandPaletteView {}

    unsafe impl NSSearchFieldDelegate for CommandPaletteView {}

    unsafe impl NSTableViewDataSource for CommandPaletteView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.quick_result_count()
        }
    }

    unsafe impl NSTableViewDelegate for CommandPaletteView {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn __selection_did_change(&self, _notification: &NSNotification) {
            let row = self.ivars().table_view.selectedRow();
            self.ivars().model.borrow_mut().select(row);
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn __should_select_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            row >= 0 && row < self.quick_result_count()
        }

        #[unsafe(method(tableViewSelectionIsChanging:))]
        fn __selection_is_changing(&self, _notification: &NSNotification) {
            let row = self.ivars().table_view.selectedRow();
            if row >= 0 {
                self.ivars().model.borrow_mut().select(row);
            }
        }

        #[unsafe(method(tableView:shouldTypeSelectForEvent:withCurrentSearchString:))]
        fn __should_type_select(
            &self,
            _table_view: &NSTableView,
            _event: &NSEvent,
            _search: Option<&NSString>,
        ) -> bool {
            false
        }
    }
);

impl PanelSurface for CommandPaletteView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::WIDE_WIDTH
    }
}

impl CommandPaletteView {
    /// `CommandPaletteView()`: the current style sheet and the user-defaults
    /// recents store.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<CommandPaletteView> {
        Self::new(
            Rc::new(StyleSheet::current(mtm)),
            Rc::new(UserDefaultsCommandPaletteRecentStore::standard()),
            None,
            Vec::new(),
            mtm,
        )
    }

    /// `init(styleSheet:recentStore:model:providers:)`.
    pub fn new(
        style_sheet: Rc<StyleSheet>,
        recent_store: Rc<dyn CommandPaletteRecentStore>,
        model: Option<CommandPaletteModel>,
        providers: Vec<Rc<dyn QuickOpenProvider>>,
        mtm: MainThreadMarker,
    ) -> Retained<CommandPaletteView> {
        // Stored-property initial values, in declaration order, then the
        // init body's assignments.
        let search_field = NSSearchField::new(mtm);
        let table_view = NSTableView::new(mtm);
        let scroll_view = NSScrollView::new(mtm);
        let hint_label = label("↑ ↓ Move   Return Open   Esc Close", mtm);
        let prefix_label = label("> commands   @ symbols   # headings   task: tasks   file:   link:   asset:", mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let backdrop = PanelBackdrop::new(
            style_sheet.clone(),
            NSVisualEffectMaterial::HUDWindow,
            NSVisualEffectBlendingMode::WithinWindow,
            mtm,
        );
        let model = model.unwrap_or_else(|| {
            CommandPaletteModel::with_commands(
                &Command::ALL_CASES,
                CommandPaletteModel::store_bindings,
                recent_store.recent_commands(),
                providers,
            )
        });
        let this = Self::alloc(mtm).set_ivars(CommandPaletteViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            backdrop,
            search_field,
            table_view,
            scroll_view,
            hint_label,
            prefix_label,
            empty_state,
            model: RefCell::new(model),
            recent_store,
            key_monitor: RefCell::new(None),
            actions: RefCell::new(Vec::new()),
        });
        let this: Retained<CommandPaletteView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.build_interface();
        this.reload_results();
        this
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn CommandPaletteViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn CommandPaletteViewDelegate>>) {
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

    /// `preferredWidth`.
    pub fn preferred_width(&self) -> CGFloat {
        PanelMetrics::WIDE_WIDTH
    }

    // MARK: - Building

    fn build_interface(&self) {
        let mtm = self.mtm();
        let ivars = self.ivars();
        self.setWantsLayer(true);
        if let Some(layer) = self.layer() {
            layer.setCornerRadius(PanelMetrics::CORNER_RADIUS);
            layer.setMasksToBounds(true);
        }

        let backdrop = &ivars.backdrop;
        backdrop.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(backdrop);
        activate(&[
            backdrop.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            backdrop.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            backdrop.topAnchor().constraintEqualToAnchor(&self.topAnchor()),
            backdrop.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);

        let search_field = &ivars.search_field;
        search_field.setPlaceholderString(Some(&ns_string("Search commands, headings, files")));
        search_field.setSendsSearchStringImmediately(true);
        search_field.setControlSize(NSControlSize::Large);
        search_field.setFont(Some(&PanelFont::system_regular(16.0)));
        search_field.setBezeled(false);
        search_field.setFocusRingType(NSFocusRingType::None);
        unsafe { search_field.setDelegate(Some(ProtocolObject::from_ref(self))) };
        set_label(&**search_field, "Search commands and document items");
        set_help(&**search_field, "Type a command, heading, task, link, asset, or file");
        search_field.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(search_field);

        let filter_chips: [(&str, &str); 6] = [
            ("All", ""),
            ("Commands", "> "),
            ("Headings", "# "),
            ("Symbols", "@ "),
            ("Tasks", "task: "),
            ("Files", "file: "),
        ];
        let filter_stack = NSStackView::new(mtm);
        filter_stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        filter_stack.setSpacing(6.0);
        filter_stack.setAlignment(NSLayoutAttribute::CenterY);
        filter_stack.setTranslatesAutoresizingMaskIntoConstraints(false);

        for (title, prefix) in filter_chips {
            let weak: ObjcWeak<CommandPaletteView> = ObjcWeak::from(self);
            let action = ButtonAction::new(
                move || {
                    let Some(this) = weak.load() else { return };
                    this.apply_filter_chip(prefix);
                },
                mtm,
            );
            ivars.actions.borrow_mut().push(action.clone());
            let button = ToolbarInteractiveButton::new(NSRect::ZERO, mtm);
            let font = PanelFont::system(11.0, weight_medium());
            let color = self.style_sheet().text_secondary.clone();
            button.setAttributedTitle(&upleft_render::appkit_compat::attributed_string(
                title,
                &[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*color))],
            ));
            button.set_feedback_inset_x(4.0);
            button.set_feedback_inset_y(2.0);
            button.set_feedback_corner_radius(4.0);
            button.setBordered(false);
            button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
            unsafe { button.setTarget(Some(object(&*action))) };
            unsafe { button.setAction(Some(ButtonAction::selector())) };
            filter_stack.addArrangedSubview(&button);
        }
        self.addSubview(&filter_stack);

        let table_view = &ivars.table_view;
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns_string("command"));
        table_view.addTableColumn(&column);
        table_view.setHeaderView(None);
        unsafe { table_view.setDelegate(Some(ProtocolObject::from_ref(self))) };
        unsafe { table_view.setDataSource(Some(ProtocolObject::from_ref(self))) };
        unsafe { table_view.setTarget(Some(object(self))) };
        unsafe { table_view.setDoubleAction(Some(sel!(doubleClick:))) };
        table_view.setRowHeight(PanelMetrics::DETAIL_ROW_HEIGHT);
        table_view.setIntercellSpacing(NSSize::new(0.0, 1.0));
        table_view.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::Regular);
        table_view.setBackgroundColor(&NSColor::clearColor());
        set_label(&**table_view, "Command results");
        set_role(&**table_view, role::list());
        table_view.setTranslatesAutoresizingMaskIntoConstraints(false);

        let scroll_view = &ivars.scroll_view;
        scroll_view.setDocumentView(Some(table_view));
        scroll_view.setHasVerticalScroller(true);
        scroll_view.setDrawsBackground(false);
        scroll_view.setBorderType(NSBorderType::NoBorder);
        scroll_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(scroll_view);

        let hint_label = &ivars.hint_label;
        hint_label.setFont(Some(&PanelFont::secondary()));
        hint_label.setAlignment(NSTextAlignment::Right);
        hint_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        hint_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        set_label(&**hint_label, "Keyboard commands: move, open, close");
        self.addSubview(hint_label);

        let prefix_label = &ivars.prefix_label;
        prefix_label.setFont(Some(&PanelFont::system_regular(10.5)));
        prefix_label.setAlignment(NSTextAlignment::Left);
        prefix_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        prefix_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        prefix_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        set_label(
            &**prefix_label,
            &("Search prefixes: greater-than for commands, at for symbols, hash for headings, ".to_owned()
                + "hash task for tasks, file colon, link colon, asset colon"),
        );
        self.addSubview(prefix_label);
        ivars.empty_state.install(self, scroll_view, 1.0);

        activate(&[
            search_field.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            search_field.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            search_field.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 14.0),
            search_field.heightAnchor().constraintEqualToConstant(32.0),
            filter_stack.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            filter_stack.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            filter_stack.topAnchor().constraintEqualToAnchor_constant(&search_field.bottomAnchor(), 6.0),
            filter_stack.heightAnchor().constraintEqualToConstant(20.0),
            scroll_view.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 8.0),
            scroll_view.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -8.0),
            scroll_view.topAnchor().constraintEqualToAnchor_constant(&filter_stack.bottomAnchor(), 6.0),
            scroll_view.bottomAnchor().constraintEqualToAnchor_constant(&hint_label.topAnchor(), -8.0),
            prefix_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            prefix_label.centerYAnchor().constraintEqualToAnchor(&hint_label.centerYAnchor()),
            hint_label.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&prefix_label.trailingAnchor(), 12.0),
            hint_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            hint_label.bottomAnchor().constraintEqualToAnchor_constant(&self.bottomAnchor(), -10.0),
            hint_label.heightAnchor().constraintEqualToConstant(16.0),
        ]);

        set_role(self, role::group());
        set_label(self, "Command palette");
        self.apply_style();
    }

    /// A filter chip's action: the prefix becomes the query.
    fn apply_filter_chip(&self, prefix: &str) {
        let ivars = self.ivars();
        ivars.search_field.setStringValue(&ns_string(prefix));
        ivars.model.borrow_mut().update_query(prefix);
        self.reload_results();
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&ivars.search_field));
        }
        if let Some(editor) = ivars.search_field.currentEditor() {
            editor.setSelectedRange(NSRange::new(swift_text::count(prefix) as usize, 0));
        }
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.background)));
        }
        ivars.search_field.setBackgroundColor(Some(&style_sheet.background));
        ivars.search_field.setTextColor(Some(&style_sheet.text));
        ivars.hint_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.prefix_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.table_view.reloadData();
        self.update_empty_state();
    }

    fn reload_results(&self) {
        let ivars = self.ivars();
        let table_view = &ivars.table_view;
        table_view.reloadData();
        let count = self.quick_result_count();
        if count > 0 {
            let selected_index = ivars.model.borrow().selected_index();
            table_view.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(selected_index as usize),
                false,
            );
            table_view.scrollRowToVisible(selected_index);
        } else {
            unsafe { table_view.deselectAll(None) };
        }
        let status = if count == 1 { "1 command".to_owned() } else { format!("{count} commands") };
        set_value(&**table_view, &status);
        self.update_empty_state();
    }

    /// A typo produced a blank rectangle. Say what happened and how to get
    /// out of it instead (§11.4).
    fn update_empty_state(&self) {
        let ivars = self.ivars();
        if self.quick_result_count() != 0 {
            ivars.empty_state.setHidden(true);
            ivars.scroll_view.setHidden(false);
            return;
        }
        let value = ivars.search_field.stringValue().to_string();
        let query = swift_text::trim_whitespaces_and_newlines(&value);
        let title = if query.is_empty() { "Nothing to open yet".to_owned() } else { format!("No matches for “{query}”") };
        ivars.empty_state.configure(
            "magnifyingglass",
            &title,
            "Try a prefix: `>` for commands,\n`@` for symbols, `#` for headings.",
            &self.style_sheet(),
        );
        ivars.empty_state.setHidden(false);
        ivars.scroll_view.setHidden(true);
    }

    fn install_key_monitor(&self) {
        let weak: ObjcWeak<CommandPaletteView> = ObjcWeak::from(self);
        let block = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
            let Some(this) = weak.load() else { return event.as_ptr() };
            // SAFETY: AppKit hands the monitor a live event.
            let event_ref = unsafe { event.as_ref() };
            let same_window = match (event_ref.window(this.mtm()), this.window()) {
                (Some(a), Some(b)) => std::ptr::eq(Retained::as_ptr(&a), Retained::as_ptr(&b)),
                (None, None) => true,
                _ => false,
            };
            if !same_window {
                return event.as_ptr();
            }
            if this.handle_key(event_ref) { std::ptr::null_mut() } else { event.as_ptr() }
        });
        let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
        *self.ivars().key_monitor.borrow_mut() = monitor;
    }

    fn remove_key_monitor(&self) {
        let monitor = self.ivars().key_monitor.borrow_mut().take();
        if let Some(monitor) = monitor {
            // SAFETY: the monitor came from `addLocalMonitorForEvents`.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }

    fn handle_key(&self, event: &NSEvent) -> bool {
        match event.keyCode() {
            125 => {
                self.ivars().model.borrow_mut().move_selection(1);
                self.reload_results();
                true
            }
            126 => {
                self.ivars().model.borrow_mut().move_selection(-1);
                self.reload_results();
                true
            }
            36 | 76 => {
                self.choose_selection();
                true
            }
            53 => {
                if let Some(delegate) = self.delegate() {
                    delegate.command_palette_did_cancel(self);
                }
                true
            }
            _ => false,
        }
    }

    fn choose_selection(&self) {
        let ivars = self.ivars();
        let Some(result) = ivars.model.borrow().selected_result() else { return };
        if let QuickOpenAction::Command(command) = result.action {
            ivars.model.borrow_mut().record(command);
            ivars.recent_store.record(command);
        }
        if let Some(delegate) = self.delegate() {
            delegate.command_palette_did_choose(self, &result);
        }
    }

    // MARK: - Table

    fn quick_result_count(&self) -> isize {
        self.ivars().model.borrow().with_quick_results(|results| results.len() as isize)
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let result = self.ivars().model.borrow().with_quick_results(|results| {
            usize::try_from(row).ok().and_then(|row| results.get(row).cloned())
        })?;
        // Reuse: this table reloads on every keystroke.
        let identifier = ns_string("commandPaletteRow");
        let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
            .and_then(|view| downcast::<CommandPaletteRowView>(&view))
            .unwrap_or_else(|| CommandPaletteRowView::new(&identifier, self.mtm()));
        let query = self.ivars().search_field.stringValue().to_string();
        cell.configure(&result, &query, &self.style_sheet());
        Some(Retained::into_super(Retained::into_super(cell)))
    }
}

// MARK: - CommandPaletteRowView

pub struct CommandPaletteRowViewIvars {
    title_label: Retained<NSTextField>,
    metadata_label: Retained<NSTextField>,
}

define_class!(
    /// `CommandPaletteRowView` (private in Swift).
    // SAFETY: `initWithFrame:` sets the ivars, so AppKit's own instantiation
    // paths create a valid instance too.
    #[unsafe(super(NSTableCellView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "CommandPaletteRowView"]
    #[ivars = CommandPaletteRowViewIvars]
    struct CommandPaletteRowView;

    unsafe impl NSObjectProtocol for CommandPaletteRowView {}

    impl CommandPaletteRowView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("CommandPaletteRowView is created on the main thread");
            let this = this.set_ivars(CommandPaletteRowViewIvars {
                title_label: label("", mtm),
                metadata_label: label("", mtm),
            });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }
    }
);

impl CommandPaletteRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<CommandPaletteRowView> {
        let this: Retained<CommandPaletteRowView> =
            unsafe { msg_send![CommandPaletteRowView::alloc(mtm), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        let ivars = this.ivars();
        let title_label = &ivars.title_label;
        let metadata_label = &ivars.metadata_label;
        title_label.setFont(Some(&PanelFont::row_emphasised()));
        metadata_label.setFont(Some(&PanelFont::secondary()));
        // Long paths used to be cut off mid-glyph with no ellipsis.
        title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        metadata_label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        metadata_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(title_label);
        this.addSubview(metadata_label);
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 12.0),
            title_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -12.0),
            title_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 6.0),
            metadata_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            metadata_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -12.0),
            metadata_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 2.0),
            metadata_label.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&this.bottomAnchor(), -5.0),
        ]);
        set_role(&*this, role::static_text());
        this
    }

    fn configure(&self, result: &QuickOpenResult, query: &str, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        let metadata_label = &ivars.metadata_label;
        let mut clean_query = swift_text::trim_whitespaces_and_newlines(query).to_owned();
        for token in [">", "@", "#", "task:", "file:"] {
            clean_query = swift_text::replacing_occurrences(&clean_query, token, "");
        }
        let clean_query = swift_text::trim_whitespaces_and_newlines(&clean_query).to_owned();
        let matched = if clean_query.is_empty() { None } else { FuzzyMatcher::r#match(&clean_query, &result.title) };
        if let Some(matched) = matched {
            let font = PanelFont::row_emphasised();
            let attributes =
                attributes_dictionary(&[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*style_sheet.text))]);
            let attr = unsafe {
                NSMutableAttributedString::initWithString_attributes(
                    NSMutableAttributedString::alloc(),
                    &ns_string(&result.title),
                    Some(&attributes),
                )
            };
            for idx in matched.positions {
                if idx < attr.length() as isize {
                    unsafe {
                        attr.addAttribute_value_range(
                            keys::foreground_color(),
                            object(&*style_sheet.accent),
                            NSRange::new(idx as usize, 1),
                        )
                    };
                }
            }
            title_label.setAttributedStringValue(&attr);
        } else {
            title_label.setStringValue(&ns_string(&result.title));
            title_label.setTextColor(Some(&style_sheet.text));
        }
        let metadata = if result.subtitle.is_empty() {
            swift_text::capitalized(result.kind.raw_value())
        } else {
            result.subtitle.clone()
        };
        metadata_label.setStringValue(&ns_string(&metadata));
        metadata_label.setTextColor(Some(&style_sheet.text_faint));
        let tip = if result.subtitle.is_empty() {
            result.title.clone()
        } else {
            format!("{}\n{}", result.title, result.subtitle)
        };
        self.setToolTip(Some(&ns_string(&tip)));
        set_label(self, &result.title);
        let value = metadata_label.stringValue();
        let _: () = unsafe { msg_send![self, setAccessibilityValue: &*value] };
    }
}

