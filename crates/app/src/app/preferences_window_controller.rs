//! Port of `App/PreferencesWindowController.swift`: the Settings window, its
//! panes, the declarative form rows behind them, and the keyboard-shortcut
//! recorder.
//!
//! The Swift file is split across private submodules, re-exported here:
//!
//! | Swift | Rust | Objective-C class |
//! |---|---|---|
//! | `SettingsPane` | [`SettingsPane`] | — |
//! | `PreferenceSearchable` | [`PreferenceSearchable`] (trait) | — |
//! | `PreferencesWindowController` | [`PreferencesWindowController`] | `PreferencesWindowController` |
//! | `PreferenceRow`, `PreferenceRow.ChoiceSelection` | [`PreferenceRow`], [`ChoiceSelection`] (`preference_row`) | — |
//! | `PreferenceRowFilter` | [`PreferenceRowFilter`] (`preference_row`) | — |
//! | `FlippedStackView` (private) | `preferences_pane::FlippedStackView` | `FlippedStackView` |
//! | `ThemePreviewView` (private) | `preferences_pane::ThemePreviewView` | `ThemePreviewView` |
//! | `PreferencesPane` | [`PreferencesPane`] (`preferences_pane`) | `PreferencesPane` |
//! | `ActionHandler` | [`ActionHandler`] (`preferences_pane`) | `ActionHandler` |
//! | `PreferencesForms` | [`PreferencesForms`] (`preferences_forms`) | — |
//! | `KeybindingsPane` | [`KeybindingsPane`] (`keybindings_pane`) | `KeybindingsPane` |
//!
//! `tabs.observe(\.selectedTabViewItemIndex)` is a key-value observation
//! whose observer is `PreferencesTabSelectionObservation`, a private
//! `NSObject` subclass standing in for Foundation's `NSKeyValueObservation`.
//!
//! Threading follows the Swift: every pane builds its rows on the main thread
//! when it loads or appears (they read `Preferences`, LaunchServices, the
//! snapshot store's size and the font list), because the rows are what the
//! pane's first frame shows.

mod keybindings_pane;
mod preference_row;
mod preferences_forms;
mod preferences_pane;

use std::cell::RefCell;
use std::ffi::c_void;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSImage, NSLayoutAttribute, NSLayoutConstraint, NSResponder, NSSearchField, NSTabViewController,
    NSTabViewControllerTabStyle, NSTabViewItem, NSTitlebarAccessoryViewController, NSView, NSViewController, NSWindow,
    NSWindowController, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSKeyValueObservingOptions, NSObjectNSKeyValueObserverRegistration, NSSize, NSString, NSUserDefaults,
};
use upleft_swift_text as swift_text;

pub use keybindings_pane::KeybindingsPane;
pub use preference_row::{ChoiceSelection, PreferenceRow, PreferenceRowFilter};
pub use preferences_forms::PreferencesForms;
pub use preferences_pane::{ActionHandler, PreferencesPane};

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

/// `SettingsPane`: the panes of the Settings window, in the order they
/// appear.
///
/// A typed value rather than a title string: "Keyboard Shortcuts…" has to be
/// able to name the pane it opens, and the search field has to be able to
/// jump to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsPane {
    General,
    Appearance,
    Typography,
    Editor,
    History,
    Updates,
    Keys,
}

impl SettingsPane {
    /// `SettingsPane.allCases`.
    pub const ALL_CASES: [SettingsPane; 7] = [
        SettingsPane::General,
        SettingsPane::Appearance,
        SettingsPane::Typography,
        SettingsPane::Editor,
        SettingsPane::History,
        SettingsPane::Updates,
        SettingsPane::Keys,
    ];

    pub const fn raw_value(self) -> &'static str {
        match self {
            SettingsPane::General => "general",
            SettingsPane::Appearance => "appearance",
            SettingsPane::Typography => "typography",
            SettingsPane::Editor => "editor",
            SettingsPane::History => "history",
            SettingsPane::Updates => "updates",
            SettingsPane::Keys => "keys",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<SettingsPane> {
        SettingsPane::ALL_CASES.into_iter().find(|pane| pane.raw_value() == raw)
    }

    pub const fn title(self) -> &'static str {
        match self {
            SettingsPane::General => "General",
            SettingsPane::Appearance => "Appearance",
            SettingsPane::Typography => "Typography",
            SettingsPane::Editor => "Editor",
            SettingsPane::History => "History",
            SettingsPane::Updates => "Updates",
            SettingsPane::Keys => "Keys",
        }
    }

    pub const fn symbol(self) -> &'static str {
        match self {
            SettingsPane::General => "gearshape",
            SettingsPane::Appearance => "circle.lefthalf.filled",
            SettingsPane::Typography => "textformat",
            SettingsPane::Editor => "square.and.pencil",
            SettingsPane::History => "clock.arrow.circlepath",
            SettingsPane::Updates => "arrow.down.circle",
            SettingsPane::Keys => "keyboard",
        }
    }
}

/// `PreferenceSearchable`: a pane that participates in the settings search.
pub trait PreferenceSearchable {
    /// Words the user typed; empty means "show everything".
    fn search_query(&self) -> String;
    /// `searchQuery = …`.
    fn set_search_query(&self, query: &str);
    /// How many rows survive the current query.
    fn search_match_count(&self) -> isize;
}

/// `controller as? PreferenceSearchable`.
fn searchable(controller: &NSViewController) -> Option<&dyn PreferenceSearchable> {
    if let Some(pane) = controller.downcast_ref::<PreferencesPane>() {
        return Some(pane);
    }
    if let Some(pane) = controller.downcast_ref::<KeybindingsPane>() {
        return Some(pane);
    }
    None
}

// MARK: - Key-value observation

define_class!(
    /// The observer behind `tabs.observe(\.selectedTabViewItemIndex,
    /// options: [.new])`: it runs its closure for every change.
    // SAFETY: `init` is forwarded in `KeyValueObservation::new` after the
    // ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PreferencesTabSelectionObservation"]
    #[ivars = Box<dyn Fn()>]
    struct PreferencesTabSelectionObservation;

    impl PreferencesTabSelectionObservation {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn __observe(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            (self.ivars())();
        }
    }
);

/// `NSKeyValueObservation`: registered on creation, removed when dropped.
struct KeyValueObservation {
    observer: Retained<PreferencesTabSelectionObservation>,
    object: Weak<NSObject>,
    key_path: Retained<NSString>,
}

impl KeyValueObservation {
    fn new(
        object: &NSObject,
        key_path: &str,
        options: NSKeyValueObservingOptions,
        callback: impl Fn() + 'static,
        mtm: MainThreadMarker,
    ) -> KeyValueObservation {
        let observer = PreferencesTabSelectionObservation::alloc(mtm).set_ivars(Box::new(callback) as Box<dyn Fn()>);
        // SAFETY: NSObject's designated initialiser.
        let observer: Retained<PreferencesTabSelectionObservation> = unsafe { msg_send![super(observer), init] };
        let key_path = ns(key_path);
        // SAFETY: the observer implements `observeValueForKeyPath:…` and is
        // removed in `drop`, before it is released.
        unsafe { object.addObserver_forKeyPath_options_context(&observer, &key_path, options, std::ptr::null_mut()) };
        KeyValueObservation { observer, object: Weak::from(object), key_path }
    }
}

impl Drop for KeyValueObservation {
    /// `NSKeyValueObservation.invalidate()`, which its `deinit` calls.
    fn drop(&mut self) {
        if let Some(object) = self.object.load() {
            // SAFETY: the observer was registered for this key path in `new`.
            unsafe { object.removeObserver_forKeyPath(&self.observer, &self.key_path) };
        }
    }
}

// MARK: - PreferencesWindowController

pub struct PreferencesWindowControllerIvars {
    /// First, so the observation is removed before anything else goes.
    tab_selection_observer: RefCell<Option<KeyValueObservation>>,
    tabs: Retained<NSTabViewController>,
    panes: RefCell<Vec<(SettingsPane, Retained<NSViewController>)>>,
    search_field: Retained<NSSearchField>,
}

define_class!(
    /// `final class PreferencesWindowController: NSWindowController`:
    /// Settings, including the keybinding editor.
    // SAFETY: `initWithWindow:` is forwarded in `init_with_window` after the
    // ivars are set.
    #[unsafe(super(NSWindowController, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PreferencesWindowController"]
    #[ivars = PreferencesWindowControllerIvars]
    pub struct PreferencesWindowController;

    unsafe impl NSObjectProtocol for PreferencesWindowController {}

    impl PreferencesWindowController {
        #[unsafe(method(searchChanged:))]
        fn __search_changed(&self, sender: &NSSearchField) {
            self.search_changed(sender);
        }
    }
);

impl PreferencesWindowController {
    /// `convenience init()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<PreferencesWindowController> {
        let tabs = NSTabViewController::new(mtm);
        tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
        let window = NSWindow::windowWithContentViewController(&tabs);
        window.setTitle(&ns("Upleft Settings"));
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::Resizable);
        window.setContentSize(NSSize::new(760.0, 620.0));
        // Resizable, but not to the point of self-harm. The forms lay out at
        // a fixed leading inset with wrapping help text under each control,
        // so a narrow window clips labels rather than reflowing them, and a
        // short one hides the keys pane's table behind its own footer. This
        // floor is the narrowest width at which the longest setting label
        // still fits on one line, and the shortest height that leaves the
        // keys table more rows than chrome.
        window.setContentMinSize(NSSize::new(620.0, 420.0));
        window.setFrameAutosaveName(&ns("DownrightSettingsWindow"));
        PreferencesWindowController::init_with_window(&window, tabs, mtm)
    }

    /// `private init(window:tabs:)`.
    fn init_with_window(
        window: &NSWindow,
        tabs: Retained<NSTabViewController>,
        mtm: MainThreadMarker,
    ) -> Retained<PreferencesWindowController> {
        let search_field = NSSearchField::new(mtm);
        let this = Self::alloc(mtm).set_ivars(PreferencesWindowControllerIvars {
            tab_selection_observer: RefCell::new(None),
            tabs: tabs.clone(),
            panes: RefCell::new(Vec::new()),
            search_field,
        });
        // SAFETY: NSWindowController's designated initialiser.
        let this: Retained<PreferencesWindowController> = unsafe { msg_send![super(this), initWithWindow: Some(window)] };

        for pane in SettingsPane::ALL_CASES {
            let controller = PreferencesWindowController::controller(pane, mtm);
            if pane == SettingsPane::Keys {
                let _ = controller.view();
            }
            controller.setTitle(Some(&ns(pane.title())));
            this.ivars().panes.borrow_mut().push((pane, controller.clone()));
            let item = NSTabViewItem::tabViewItemWithViewController(&controller);
            // `NSTabViewItem(viewController:)` copies the controller's `title`
            // once, when it is built, and never reads it again — a pane that
            // names itself later (the keys pane did, in `loadView()`, which
            // does not run until the tab is first selected) shows up in the
            // toolbar as "DownrightApp.KeybindingsPane". `SettingsPane`
            // already owns the titles, so the label is taken from there rather
            // than left to the order in which two objects happen to be
            // constructed.
            item.setLabel(&ns(pane.title()));
            // SAFETY: an NSString identifier, as Swift's `pane.rawValue`.
            unsafe { item.setIdentifier(Some(&ns(pane.raw_value()))) };
            item.setImage(
                NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(pane.symbol()), Some(&ns(pane.title())))
                    .as_deref(),
            );
            tabs.addTabViewItem(&item);
            // AppKit may re-read the controller title while adopting the item.
            item.setLabel(&ns(pane.title()));
        }
        let saved = NSUserDefaults::standardUserDefaults().integerForKey(&ns("settings.selectedPane"));
        let valid = saved >= 0 && (saved as usize) < SettingsPane::ALL_CASES.len();
        tabs.setSelectedTabViewItemIndex(if valid { saved } else { 0 });
        let weak: Weak<PreferencesWindowController> = Weak::from(&*this);
        let observation = KeyValueObservation::new(
            &tabs,
            "selectedTabViewItemIndex",
            NSKeyValueObservingOptions::New,
            move || {
                if let Some(this) = weak.load() {
                    this.selected_pane_did_change();
                }
            },
            mtm,
        );
        *this.ivars().tab_selection_observer.borrow_mut() = Some(observation);
        this.install_search_field(window);
        let selected = tabs.selectedTabViewItemIndex();
        this.resize_window(SettingsPane::ALL_CASES[selected as usize], false);
        this
    }

    /// The controller behind a pane. One place builds them, so a pane cannot
    /// be assembled one way here and another way in a test.
    pub fn controller(pane: SettingsPane, mtm: MainThreadMarker) -> Retained<NSViewController> {
        if pane == SettingsPane::Keys {
            Retained::into_super(KeybindingsPane::new(mtm))
        } else {
            Retained::into_super(PreferencesPane::new(pane, PreferencesForms::rows(pane, mtm), mtm))
        }
    }

    /// The names the tab toolbar shows, for the regression test that keeps a
    /// pane from advertising its class name.
    pub fn tab_labels_for_testing(&self) -> Vec<String> {
        self.ivars().tabs.tabViewItems().iter().map(|item| item.label().to_string()).collect()
    }

    /// `select(_:)`.
    pub fn select(&self, pane: SettingsPane) {
        let index = self.ivars().panes.borrow().iter().position(|(entry, _)| *entry == pane);
        let Some(index) = index else { return };
        self.ivars().tabs.setSelectedTabViewItemIndex(index as isize);
    }

    fn selected_pane_did_change(&self) {
        let index = self.ivars().tabs.selectedTabViewItemIndex();
        if !(index >= 0 && (index as usize) < SettingsPane::ALL_CASES.len()) {
            return;
        }
        NSUserDefaults::standardUserDefaults().setInteger_forKey(index, &ns("settings.selectedPane"));
        self.resize_window(SettingsPane::ALL_CASES[index as usize], true);
    }

    fn resize_window(&self, pane: SettingsPane, _animated: bool) {
        let height: f64 = match pane {
            SettingsPane::Appearance | SettingsPane::Updates => 460.0,
            SettingsPane::History => 500.0,
            SettingsPane::General | SettingsPane::Editor | SettingsPane::Typography => 620.0,
            SettingsPane::Keys => 680.0,
        };
        if let Some(window) = self.window() {
            window.setContentSize(NSSize::new(760.0, height));
        }
    }

    // MARK: - Search

    /// Seven panes and about thirty controls is more than anyone should have
    /// to scan by eye. The rows are already declarative data, so filtering
    /// them costs one pass — and because every pane filters at once, a query
    /// also tells the user which pane the setting lives in.
    fn install_search_field(&self, window: &NSWindow) {
        let mtm = MainThreadMarker::from(self);
        let search_field = &self.ivars().search_field;
        search_field.setTranslatesAutoresizingMaskIntoConstraints(false);
        search_field.setPlaceholderString(Some(&ns("Search settings")));
        search_field.setSendsSearchStringImmediately(false);
        search_field.setSendsWholeSearchString(false);
        // SAFETY: the controller owns the field and outlives it as its target.
        unsafe {
            search_field.setTarget(Some(self));
            search_field.setAction(Some(sel!(searchChanged:)));
        }

        let container = NSView::new(mtm);
        container.setTranslatesAutoresizingMaskIntoConstraints(false);
        container.addSubview(search_field);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            container.heightAnchor().constraintEqualToConstant(38.0),
            search_field.leadingAnchor().constraintEqualToAnchor_constant(&container.leadingAnchor(), 24.0),
            search_field.trailingAnchor().constraintEqualToAnchor_constant(&container.trailingAnchor(), -24.0),
            search_field.centerYAnchor().constraintEqualToAnchor(&container.centerYAnchor()),
        ]));

        let accessory = NSTitlebarAccessoryViewController::new(mtm);
        accessory.setView(&container);
        accessory.setLayoutAttribute(NSLayoutAttribute::Bottom);
        window.addTitlebarAccessoryViewController(&accessory);
    }

    fn search_changed(&self, sender: &NSSearchField) {
        let value = sender.stringValue().to_string();
        let query = swift_text::trim_whitespaces(&value).to_owned();
        let panes = self.ivars().panes.borrow().clone();
        for (_, controller) in &panes {
            if let Some(searchable) = searchable(controller) {
                searchable.set_search_query(&query);
            }
        }
        if query.is_empty() {
            return;
        }
        // Jump to the first pane that has something to show, so a query that
        // matches one setting lands the user on it rather than on an empty
        // pane.
        let hit = panes
            .iter()
            .position(|(_, controller)| searchable(controller).map(|pane| pane.search_match_count() > 0).unwrap_or(false));
        if let Some(hit) = hit {
            self.ivars().tabs.setSelectedTabViewItemIndex(hit as isize);
        }
    }
}
