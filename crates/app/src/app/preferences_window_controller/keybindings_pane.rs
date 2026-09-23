//! `KeybindingsPane` (PreferencesWindowController.swift, "Keybindings pane"):
//! the command table and the keyboard-shortcut recorder.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSButton, NSColor, NSControlStateValueOn,
    NSControlTextEditingDelegate, NSEvent, NSEventMask, NSFont, NSFontWeightRegular,
    NSImage, NSLayoutAttribute, NSLayoutConstraint, NSMenu, NSResponder, NSScrollView, NSStackView,
    NSTabViewController, NSTableColumn, NSTableHeaderView, NSTableView, NSTableViewDataSource, NSTableViewDelegate,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindowDidResignKeyNotification,
};
use objc2_foundation::{
    NSArray, NSBundle, NSEdgeInsets, NSIndexSet, NSNotification, NSNotificationCenter, NSOperationQueue, NSRange,
    NSString,
};
use upleft_render::appkit_compat::main_async;
use upleft_swift_text as swift_text;

use super::{PreferenceSearchable, SettingsPane};
use crate::support::commands::{Command, KeyBinding, ModifierFlags};
use crate::support::keybindings::{KeybindingDefaults, KeybindingStore};
use crate::support::preferences::Preferences;

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

/// Swift's tuple `<` on `(String, String)`: the first components decide
/// unless they are equal.
fn pair_less(lhs: (&str, &str), rhs: (&str, &str)) -> bool {
    if !swift_text::str_eq(lhs.0, rhs.0) {
        return swift_text::str_less(lhs.0, rhs.0);
    }
    swift_text::str_less(lhs.1, rhs.1)
}

pub struct KeybindingsPaneIvars {
    table: Retained<NSTableView>,
    record_button: Retained<NSButton>,
    reset_row_button: Retained<NSButton>,
    all_commands: Vec<Command>,
    commands: RefCell<Vec<Command>>,
    recording_row: Cell<Option<isize>>,
    key_monitor: RefCell<Option<Retained<AnyObject>>>,
    resign_key_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    search_query: RefCell<String>,
}

impl Drop for KeybindingsPaneIvars {
    /// `deinit`.
    fn drop(&mut self) {
        if let Some(key_monitor) = self.key_monitor.take() {
            // SAFETY: the token `addLocalMonitorForEventsMatchingMask:handler:`
            // returned.
            unsafe { NSEvent::removeMonitor(&key_monitor) };
        }
        if let Some(observer) = self.resign_key_observer.take() {
            // SAFETY: the token `addObserverForName:object:queue:usingBlock:`
            // returned.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
    }
}

define_class!(
    /// `final class KeybindingsPane: NSViewController, PreferenceSearchable`.
    ///
    /// The keys pane is generated from the `Command` table, so a command added
    /// anywhere in the app is remappable here without touching this file.
    // SAFETY: `initWithNibName:bundle:` is forwarded in `new` after the ivars
    // are set.
    #[unsafe(super(NSViewController, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeybindingsPane"]
    #[ivars = KeybindingsPaneIvars]
    pub struct KeybindingsPane;

    unsafe impl NSObjectProtocol for KeybindingsPane {}

    impl KeybindingsPane {
        #[unsafe(method(loadView))]
        fn __load_view(&self) {
            self.load_view();
        }

        #[unsafe(method(viewDidLoad))]
        fn __view_did_load(&self) {
            // SAFETY: the override calls through to NSViewController.
            let () = unsafe { msg_send![super(self), viewDidLoad] };
            let parent = self.parentViewController().and_then(|parent| parent.downcast::<NSTabViewController>().ok());
            if let Some(item) = parent.and_then(|parent| {
                parent.tabViewItems().iter().find(|item| {
                    item.viewController(MainThreadMarker::from(self)).is_some_and(|controller| {
                        std::ptr::eq(
                            &*controller as *const NSViewController,
                            self as *const KeybindingsPane as *const NSViewController,
                        )
                    })
                })
            }) {
                item.setImage(
                    NSImage::imageWithSystemSymbolName_accessibilityDescription(
                        &ns(SettingsPane::Keys.symbol()),
                        Some(&ns(SettingsPane::Keys.title())),
                    )
                    .as_deref(),
                );
            }
        }

        #[unsafe(method(viewDidAppear))]
        fn __view_did_appear(&self) {
            // SAFETY: the override calls through to NSViewController.
            let () = unsafe { msg_send![super(self), viewDidAppear] };
            self.view_did_appear();
        }

        #[unsafe(method(viewWillDisappear))]
        fn __view_will_disappear(&self) {
            // SAFETY: the override calls through to NSViewController.
            let () = unsafe { msg_send![super(self), viewWillDisappear] };
            self.cancel_recording();
            if let Some(observer) = self.ivars().resign_key_observer.borrow().as_ref() {
                // SAFETY: the token `addObserverForName:…` returned.
                unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
            }
            *self.ivars().resign_key_observer.borrow_mut() = None;
        }

        #[unsafe(method(toggleRecording))]
        fn __toggle_recording(&self) {
            self.toggle_recording();
        }

        #[unsafe(method(beginRecordingSelectedRow))]
        fn __begin_recording_selected_row(&self) {
            self.begin_recording_selected_row();
        }

        #[unsafe(method(resetSelected))]
        fn __reset_selected(&self) {
            self.reset_selected();
        }

        #[unsafe(method(resetAll))]
        fn __reset_all(&self) {
            self.reset_all();
        }

        #[unsafe(method(toggleVim:))]
        fn __toggle_vim(&self, sender: &NSButton) {
            let on = sender.state() == NSControlStateValueOn;
            Preferences::shared().update(|values| values.vim_keys = on);
            self.ivars().table.reloadData();
        }
    }

    unsafe impl NSTableViewDataSource for KeybindingsPane {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().commands.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for KeybindingsPane {}

    unsafe impl NSTableViewDelegate for KeybindingsPane {
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn __selection_did_change(&self, _notification: &NSNotification) {
            // Moving to another command abandons the recording rather than
            // pointing it somewhere the user is no longer looking.
            self.cancel_recording();
            self.refresh_buttons();
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            _table_view: &NSTableView,
            table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_column, row)
        }
    }
);

impl KeybindingsPane {
    /// `init()`. The pane names itself here rather than in `loadView()`. Its
    /// view is not loaded until the tab is first selected, and by then
    /// everything that reads a controller's title — the tab item's label
    /// above all — has already read it and settled on the class name.
    pub fn new(mtm: MainThreadMarker) -> Retained<KeybindingsPane> {
        let table = NSTableView::new(mtm);
        // SAFETY: no target or action yet.
        let record_button = unsafe { NSButton::buttonWithTitle_target_action(&ns("Record Shortcut"), None, None, mtm) };
        // SAFETY: as above.
        let reset_row_button = unsafe { NSButton::buttonWithTitle_target_action(&ns("Reset Shortcut"), None, None, mtm) };
        let all_commands = swift_text::sort::sorted_by(Command::ALL_CASES, |a, b| {
            pair_less((a.menu().raw_value(), a.title()), (b.menu().raw_value(), b.title()))
        });
        let this = Self::alloc(mtm).set_ivars(KeybindingsPaneIvars {
            table,
            record_button,
            reset_row_button,
            all_commands,
            commands: RefCell::new(Vec::new()),
            recording_row: Cell::new(None),
            key_monitor: RefCell::new(None),
            resign_key_observer: RefCell::new(None),
            search_query: RefCell::new(String::new()),
        });
        // SAFETY: NSViewController's designated initialiser, with no nib.
        let this: Retained<KeybindingsPane> =
            unsafe { msg_send![super(this), initWithNibName: None::<&NSString>, bundle: None::<&NSBundle>] };
        this.setTitle(Some(&ns(SettingsPane::Keys.title())));
        this
    }

    /// `searchQuery`.
    pub fn search_query(&self) -> String {
        self.ivars().search_query.borrow().clone()
    }

    /// `searchQuery = …` and its `didSet`.
    pub fn set_search_query(&self, query: &str) {
        let old_value = self.ivars().search_query.replace(query.to_owned());
        if swift_text::str_eq(query, &old_value) {
            return;
        }
        self.cancel_recording();
        self.apply_filter();
    }

    /// `searchMatchCount`.
    pub fn search_match_count(&self) -> isize {
        KeybindingsPane::filtered(&self.ivars().all_commands, &self.search_query()).len() as isize
    }

    /// Commands whose menu, name, or current shortcut matches every word
    /// typed.
    pub fn filtered(commands: &[Command], query: &str) -> Vec<Command> {
        let lowered = swift_text::lowercased(query);
        let words: Vec<&str> = swift_text::split_default(&lowered, ' ');
        if words.is_empty() {
            return commands.to_vec();
        }
        commands
            .iter()
            .copied()
            .filter(|&command| {
                let bindings: Vec<String> =
                    KeybindingStore::shared().bindings(command).iter().map(KeyBinding::display_string).collect();
                let bindings = bindings.join(" ");
                let text = swift_text::lowercased(&format!("{} {} {}", command.menu().title(), command.title(), bindings));
                words.iter().all(|word| swift_text::contains(&text, word))
            })
            .collect()
    }

    fn load_view(&self) {
        let mtm = MainThreadMarker::from(self);
        let ivars = self.ivars();
        // A query typed before this pane was ever shown must survive the view
        // finally loading.
        *ivars.commands.borrow_mut() = KeybindingsPane::filtered(&ivars.all_commands, &self.search_query());

        let table = &ivars.table;
        table.setHeaderView(Some(&NSTableHeaderView::new(mtm)));
        table.setUsesAlternatingRowBackgroundColors(true);
        table.setRowHeight(22.0);
        // SAFETY: the pane owns the table and outlives it.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
            table.setTarget(Some(self));
            table.setDoubleAction(Some(sel!(beginRecordingSelectedRow)));
        }
        table.setAllowsEmptySelection(true);

        for (identifier, title, width) in
            [("menu", "Menu", 90.0), ("command", "Command", 260.0), ("binding", "Shortcut", 140.0)]
        {
            let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(identifier));
            column.setTitle(&ns(title));
            column.setWidth(width);
            table.addTableColumn(&column);
        }

        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(table));
        scroll.setHasVerticalScroller(true);

        let hint = NSTextField::labelWithString(
            &ns("Select a command and press Record, or double-click its shortcut. ⌫ clears it, ⎋ stops recording."),
            mtm,
        );
        hint.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        hint.setTextColor(Some(&NSColor::tertiaryLabelColor()));

        // SAFETY: the pane is the buttons' target for as long as it lives.
        unsafe {
            ivars.record_button.setTarget(Some(self));
            ivars.record_button.setAction(Some(sel!(toggleRecording)));
            ivars.reset_row_button.setTarget(Some(self));
            ivars.reset_row_button.setAction(Some(sel!(resetSelected)));
        }
        // SAFETY: as above.
        let reset_all =
            unsafe { NSButton::buttonWithTitle_target_action(&ns("Reset All…"), Some(self), Some(sel!(resetAll)), mtm) };
        reset_all.setHasDestructiveAction(true);
        let footer = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[
                NSView::new(mtm),
                Retained::into_super(Retained::into_super(ivars.record_button.clone())),
                Retained::into_super(Retained::into_super(ivars.reset_row_button.clone())),
                Retained::into_super(Retained::into_super(reset_all)),
            ]),
            mtm,
        );
        footer.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        footer.setSpacing(8.0);

        let stack = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[
                Retained::into_super(scroll.clone()),
                Retained::into_super(Retained::into_super(hint)),
                Retained::into_super(footer),
            ]),
            mtm,
        );
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(8.0);
        stack.setEdgeInsets(NSEdgeInsets { top: 16.0, left: 16.0, bottom: 16.0, right: 16.0 });
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            scroll.widthAnchor().constraintEqualToAnchor_constant(&stack.widthAnchor(), -32.0),
            scroll.heightAnchor().constraintGreaterThanOrEqualToConstant(340.0),
            scroll.heightAnchor().constraintEqualToAnchor_constant(&stack.heightAnchor(), -86.0),
        ]));
        self.setView(&stack);
        self.refresh_buttons();
    }

    fn view_did_appear(&self) {
        self.ivars().table.reloadData();
        // A recorder that outlives the window would swallow the next
        // keystroke anywhere in the app, so it is torn down the moment focus
        // leaves.
        if let Some(window) = self.view().window() {
            let weak: Weak<KeybindingsPane> = Weak::from(self);
            let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
                if let Some(this) = weak.load() {
                    this.cancel_recording();
                }
            });
            // SAFETY: the block runs on the main queue; the token is removed
            // in `viewWillDisappear` or on drop.
            let observer = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSWindowDidResignKeyNotification),
                    Some(&window),
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            };
            *self.ivars().resign_key_observer.borrow_mut() = Some(observer);
        }
    }

    fn apply_filter(&self) {
        let filtered = KeybindingsPane::filtered(&self.ivars().all_commands, &self.search_query());
        *self.ivars().commands.borrow_mut() = filtered;
        if self.isViewLoaded() {
            self.ivars().table.reloadData();
            self.refresh_buttons();
        }
    }

    // MARK: Recording

    fn toggle_recording(&self) {
        if self.ivars().recording_row.get().is_some() {
            self.cancel_recording();
        } else {
            self.begin_recording_selected_row();
        }
    }

    fn begin_recording_selected_row(&self) {
        let table = &self.ivars().table;
        let clicked = table.clickedRow();
        let row = if clicked >= 0 { clicked } else { table.selectedRow() };
        if !(row >= 0 && row < self.ivars().commands.borrow().len() as isize) {
            return;
        }
        self.begin_recording(row);
    }

    fn begin_recording(&self, row: isize) {
        // Always tear the previous monitor down first. Two live monitors both
        // write into whichever row was recorded last, and the older one can
        // never be removed.
        self.cancel_recording();
        self.ivars().recording_row.set(Some(row));
        let table = self.ivars().table.clone();
        table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        table.reloadDataForRowIndexes_columnIndexes(
            &NSIndexSet::indexSetWithIndex(row as usize),
            &NSIndexSet::indexSetWithIndexesInRange(NSRange::new(0, 3)),
        );
        self.refresh_buttons();

        let weak: Weak<KeybindingsPane> = Weak::from(self);
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let Some(this) = weak.load() else { return event.as_ptr() };
            let Some(row) = this.ivars().recording_row.get() else { return event.as_ptr() };
            let command = {
                let commands = this.ivars().commands.borrow();
                if !(row < commands.len() as isize) {
                    return event.as_ptr();
                }
                commands[row as usize]
            };
            // SAFETY: AppKit hands the monitor a live event.
            let result = this.record(unsafe { event.as_ref() }, command);
            // `defer { self.endRecording() }`.
            this.end_recording();
            result
        });
        // SAFETY: the block returns the event or nil, as a local monitor must.
        let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
        *self.ivars().key_monitor.borrow_mut() = monitor;
    }

    /// The body of the key monitor after its `guard`, up to the deferred
    /// `endRecording()`. Every path swallows the event.
    fn record(&self, event: &NSEvent, command: Command) -> *mut NSEvent {
        let mtm = MainThreadMarker::from(self);
        if event.keyCode() == 53 {
            // ⎋ cancels
            return std::ptr::null_mut();
        }
        if event.keyCode() == 51 {
            // ⌫ clears
            KeybindingStore::shared().set_binding(None, command);
            return std::ptr::null_mut();
        }
        let Some(key) = KeyBinding::key_for_event(event) else { return std::ptr::null_mut() };
        let binding = KeyBinding::new(
            key,
            ModifierFlags(event.modifierFlags().0 as u64).intersection(ModifierFlags::DEVICE_INDEPENDENT_FLAGS_MASK),
        );

        let conflicts = KeybindingStore::shared().conflicts(&binding, command);
        if !conflicts.is_empty() {
            // A local monitor is still on the event-dispatch stack here. Tear
            // it down before presenting so the modal loop cannot feed a key
            // back into the recorder.
            self.cancel_recording();
            main_async(move || {
                let alert = NSAlert::new(mtm);
                alert.setMessageText(&ns(&format!("{} is already used", binding.display_string())));
                let titles: Vec<&str> = conflicts.iter().map(|conflict| conflict.title()).collect();
                alert.setInformativeText(&ns(&format!("Assigned to {}. Reassign it?", titles.join(", "))));
                alert.addButtonWithTitle(&ns("Reassign"));
                alert.addButtonWithTitle(&ns("Cancel"));
                if alert.runModal() != NSAlertFirstButtonReturn {
                    return;
                }
                for conflict in &conflicts {
                    KeybindingStore::shared().set_binding(None, *conflict);
                }
                KeybindingStore::shared().set_binding(Some(binding.clone()), command);
                if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
                    refresh_key_equivalents(&menu);
                }
            });
            return std::ptr::null_mut();
        }
        KeybindingStore::shared().set_binding(Some(binding), command);
        std::ptr::null_mut()
    }

    /// Stops listening without changing anything. Safe to call when nothing
    /// is being recorded, which is what makes it usable from every exit path.
    fn cancel_recording(&self) {
        let key_monitor = self.ivars().key_monitor.take();
        if let Some(key_monitor) = key_monitor {
            // SAFETY: the token `addLocalMonitorForEventsMatchingMask:handler:`
            // returned.
            unsafe { NSEvent::removeMonitor(&key_monitor) };
        }
        if self.ivars().recording_row.get().is_none() {
            return;
        }
        self.ivars().recording_row.set(None);
        if self.isViewLoaded() {
            self.ivars().table.reloadData();
            self.refresh_buttons();
        }
    }

    fn end_recording(&self) {
        self.cancel_recording();
        if let Some(menu) = NSApplication::sharedApplication(MainThreadMarker::from(self)).mainMenu() {
            refresh_key_equivalents(&menu);
        }
    }

    fn refresh_buttons(&self) {
        let ivars = self.ivars();
        let count = ivars.commands.borrow().len() as isize;
        let has_selection = ivars.table.selectedRow() >= 0 && ivars.table.selectedRow() < count;
        let recording = ivars.recording_row.get().is_some();
        ivars.record_button.setTitle(&ns(if recording { "Cancel" } else { "Record Shortcut" }));
        ivars.record_button.setEnabled(recording || has_selection);
        ivars.reset_row_button.setEnabled(has_selection && !recording);
    }

    // MARK: Resetting

    fn reset_selected(&self) {
        let row = self.ivars().table.selectedRow();
        let command = {
            let commands = self.ivars().commands.borrow();
            if !(row >= 0 && row < commands.len() as isize) {
                return;
            }
            commands[row as usize]
        };
        // Restores the shipped chord for this command. A command that ships
        // with several chords keeps only the first until `KeybindingStore`
        // learns how to drop a single override.
        let shipped = KeybindingDefaults::table().get(&command).and_then(|bindings| bindings.first().cloned());
        KeybindingStore::shared().set_binding(shipped, command);
        self.ivars().table.reloadData();
        if let Some(menu) = NSApplication::sharedApplication(MainThreadMarker::from(self)).mainMenu() {
            refresh_key_equivalents(&menu);
        }
    }

    fn reset_all(&self) {
        let mtm = MainThreadMarker::from(self);
        self.cancel_recording();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&ns("Reset every keyboard shortcut?"));
        alert.setInformativeText(&ns(
            "All of your custom shortcuts go back to the ones Upleft ships with. This can't be undone.",
        ));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.addButtonWithTitle(&ns("Reset All"));
        alert.addButtonWithTitle(&ns("Cancel"));
        if alert.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        KeybindingStore::shared().reset_to_defaults();
        self.ivars().table.reloadData();
        if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
            refresh_key_equivalents(&menu);
        }
    }

    // MARK: NSTableViewDelegate

    fn view_for(&self, table_column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
        let mtm = MainThreadMarker::from(self);
        let command = {
            let commands = self.ivars().commands.borrow();
            if !(row < commands.len() as isize) {
                return None;
            }
            commands[row as usize]
        };
        let identifier = table_column.map(|column| column.identifier().to_string());
        let recording_row = self.ivars().recording_row.get();
        let text = match identifier.as_deref() {
            Some("menu") => command.menu().title(),
            Some("command") => command.title().to_owned(),
            _ => {
                if recording_row == Some(row) {
                    "Press a shortcut…".to_owned()
                } else {
                    let bindings: Vec<String> =
                        KeybindingStore::shared().bindings(command).iter().map(KeyBinding::display_string).collect();
                    bindings.join("  ")
                }
            }
        };
        let field = NSTextField::labelWithString(&ns(&text), mtm);
        let font = if identifier.as_deref() == Some("binding") {
            // SAFETY: AppKit exports the weight as an immutable global.
            NSFont::monospacedSystemFontOfSize_weight(11.0, unsafe { NSFontWeightRegular })
        } else {
            NSFont::systemFontOfSize(12.0)
        };
        field.setFont(Some(&font));
        // Two branches with one body, as in the Swift.
        #[allow(clippy::if_same_then_else)]
        if recording_row == Some(row) && identifier.as_deref() == Some("binding") {
            field.setTextColor(Some(&NSColor::controlAccentColor()));
        } else if KeybindingStore::shared().is_overridden(command) {
            field.setTextColor(Some(&NSColor::controlAccentColor()));
        }
        Some(Retained::into_super(Retained::into_super(field)))
    }
}

impl PreferenceSearchable for KeybindingsPane {
    fn search_query(&self) -> String {
        KeybindingsPane::search_query(self)
    }

    fn set_search_query(&self, query: &str) {
        KeybindingsPane::set_search_query(self, query);
    }

    fn search_match_count(&self) -> isize {
        KeybindingsPane::search_match_count(self)
    }
}

/// `MainMenu.refreshKeyEquivalents(in:)`.
fn refresh_key_equivalents(menu: &NSMenu) {
    crate::app::main_menu::MainMenu::refresh_key_equivalents(menu);
}
