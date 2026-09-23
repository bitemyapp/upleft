//! Port of `App/MainMenu.swift`: the menu bar, built entirely from the
//! `Command` table (§7.2).
//!
//! No menu item carries a hand-written key equivalent: every shortcut shown
//! is read back out of `KeybindingStore`, so remapping a binding in Settings
//! updates the menu, and a menu item can never advertise a shortcut that
//! doesn't work.
//!
//! Every menu — including Application, Window, and Help — is built from
//! `groups(in:)`, so a command declared for a menu cannot silently fail to
//! appear. `CommandTableTests` asserts exactly that.
//!
//! Swift's `enum MainMenu` is the namespace [`MainMenu`]. The Objective-C
//! classes keep their Swift names: `UpdateCheckMenuItem`, `HelpLinkTarget`,
//! `RecentsMenuDelegate`, `ThemeMenuDelegate`. Actions Swift spells
//! `#selector(AppDelegate.foo(_:))` are plain selectors (`foo:`) that travel
//! the responder chain to whoever implements them.

use std::cell::{OnceCell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuDelegate,
    NSMenuItem, NSMenuItemImportFromDeviceIdentifier, NSUserInterfaceItemIdentification, NSWorkspace,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue, NSString, NSURL};
use upleft_render::render_contracts::{Theme, ThemeAppearance};
use upleft_render::theme::theme_store::ThemeStore;
use upleft_swift_text as swift_text;

use crate::ai::document_state_store::DocumentStateStore;
use crate::support::commands::{Command, CommandContext, Menu, ModifierFlags};
use crate::support::keybindings::KeybindingStore;
use crate::support::preferences::Preferences;
use crate::updater::update_coordinator::UpdateCoordinator;

/// `enum MainMenu`: main-actor by declaration, so every function takes the
/// main-thread marker.
pub struct MainMenu;

impl MainMenu {
    pub fn build(mtm: MainThreadMarker) -> Retained<NSMenu> {
        // Touching the shared instance starts the observation that
        // revalidates "Check for Updates…" when the updater's state changes.
        let _ = UpdateCheckMenuItem::shared(mtm);
        let root = NSMenu::new(mtm);
        for menu in Menu::ALL_CASES {
            root.addItem(&Self::menu_item(menu, mtm));
        }
        root
    }

    /// Commands deliberately kept out of the menu bar. Empty today; it
    /// exists so that hiding a command is a decision recorded here rather
    /// than an omission nobody notices. `CommandTableTests` allows exactly
    /// these.
    pub const COMMANDS_HIDDEN_FROM_MENU_BAR: &'static [Command] = &[];

    /// Grouping inside each menu. Kept explicit rather than derived from the
    /// enum's declaration order so separators land where they read well.
    fn groups(menu: Menu, mtm: MainThreadMarker) -> Vec<MenuGroup> {
        use Command::*;
        use MenuGroup::{Commands, DynamicSubmenu, ImportFromDevice, Services, Standard, StandardSubmenu, Submenu};
        let command = ModifierFlags::COMMAND;
        let shift = ModifierFlags::SHIFT;
        let option = ModifierFlags::OPTION;
        let control = ModifierFlags::CONTROL;
        match menu {
            Menu::Application => vec![
                Standard(vec![StandardItem::new("About Upleft", sel!(orderFrontStandardAboutPanel:))]),
                // Follows About per the updater spec. Validation now comes
                // from the command's precondition, not a bespoke target.
                Commands(vec![CheckForUpdates]),
                Commands(vec![Command::Preferences, ShowKeybindings, ToggleLightDark]),
                Services,
                Standard(vec![
                    StandardItem::new("Hide Upleft", sel!(hide:)).key("h"),
                    StandardItem::new("Hide Others", sel!(hideOtherApplications:))
                        .key("h")
                        .modifiers(command | option),
                    StandardItem::new("Show All", sel!(unhideAllApplications:)),
                ]),
                Standard(vec![StandardItem::new("Quit Upleft", sel!(terminate:)).key("q")]),
            ],

            Menu::File => vec![
                Commands(vec![NewDocument, Open]),
                DynamicSubmenu {
                    title: "Open Recent",
                    delegate: ProtocolObject::from_retained(RecentsMenuDelegate::shared(mtm)),
                },
                Commands(vec![Save, SaveAs, Close]),
                // The app's own version history. macOS's Revert To / Browse
                // All Versions are NSDocument features Upleft does not use.
                Commands(vec![VersionTimeline]),
                Commands(vec![Share, ShareAsPdf]),
                // Quick Look sits with the other "look at this file" items and
                // above them, the way Finder's File menu orders it.
                Commands(vec![QuickLook, RevealInFinder, OpenInEditor, CompareFiles]),
                Standard(vec![
                    StandardItem::new("Page Setup…", sel!(runPageLayout:)).key("p").modifiers(command | shift),
                ]),
                Commands(vec![PrintDocument, ExportPdf, ExportHtml, ExportSelectionAsImage]),
            ],

            Menu::Edit => vec![
                Standard(vec![
                    StandardItem::new("Undo", sel!(undo:)).key("z"),
                    StandardItem::new("Redo", sel!(redo:)).key("z").modifiers(command | shift),
                ]),
                Standard(vec![
                    StandardItem::new("Cut", sel!(cut:)).key("x"),
                    StandardItem::new("Copy", sel!(copy:)).key("c"),
                    StandardItem::new("Paste", sel!(paste:)).key("v"),
                    StandardItem::new("Paste as Markdown", sel!(pasteAsMarkdown:)).key(""),
                    StandardItem::new("Paste and Match Style", sel!(pasteAndMatchStyle:))
                        .key("v")
                        .modifiers(command | shift),
                    StandardItem::new("Delete", sel!(delete:)),
                    StandardItem::new("Select All", sel!(selectAll:)).key("a"),
                ]),
                Commands(vec![CopyAsMarkdown, CopyAsRichText, CopyAsPlainText, CopySection, CopySectionLink]),
                Submenu {
                    title: "Find",
                    commands: vec![Find, FindNext, FindPrevious, UseSelectionForFind, FindReplace, FindInSiblings],
                },
                StandardSubmenu {
                    title: "Spelling and Grammar",
                    items: vec![
                        StandardItem::new("Show Spelling and Grammar", sel!(showGuessPanel:)).key(":"),
                        StandardItem::new("Check Document Now", sel!(checkSpelling:)).key(";"),
                        StandardItem::separator(),
                        StandardItem::new("Check Spelling While Typing", sel!(toggleContinuousSpellChecking:)),
                        StandardItem::new("Check Grammar With Spelling", sel!(toggleGrammarChecking:)),
                        StandardItem::new("Correct Spelling Automatically", sel!(toggleAutomaticSpellingCorrection:)),
                    ],
                },
                // Off by default so source bytes survive typing (§6.4); this
                // is where the user turns them on if they want them.
                StandardSubmenu {
                    title: "Substitutions",
                    items: vec![
                        StandardItem::new("Show Substitutions", sel!(orderFrontSubstitutionsPanel:)),
                        StandardItem::separator(),
                        StandardItem::new("Smart Copy/Paste", sel!(toggleSmartInsertDelete:)),
                        StandardItem::new("Smart Quotes", sel!(toggleAutomaticQuoteSubstitution:)),
                        StandardItem::new("Smart Dashes", sel!(toggleAutomaticDashSubstitution:)),
                        StandardItem::new("Smart Links", sel!(toggleAutomaticLinkDetection:)),
                        StandardItem::new("Data Detectors", sel!(toggleAutomaticDataDetection:)),
                        StandardItem::new("Text Replacement", sel!(toggleAutomaticTextReplacement:)),
                    ],
                },
                StandardSubmenu {
                    title: "Transformations",
                    items: vec![
                        StandardItem::new("Make Upper Case", sel!(uppercaseWord:)),
                        StandardItem::new("Make Lower Case", sel!(lowercaseWord:)),
                        StandardItem::new("Capitalize", sel!(capitalizeWord:)),
                    ],
                },
                ImportFromDevice,
                Submenu { title: "Speech", commands: vec![SpeakDocument, StopSpeaking] },
            ],

            Menu::Format => vec![
                Commands(vec![ToggleBold, ToggleItalic, ToggleStrikethrough, ToggleInlineCode, InsertLink]),
                Commands(vec![
                    ConvertToParagraph,
                    ConvertToBulletList,
                    ConvertToNumberedList,
                    ConvertToTaskList,
                    ConvertToBlockquote,
                ]),
                Commands(vec![IndentList, OutdentList, ToggleTaskAtCaret]),
                Commands(vec![
                    PromoteHeading,
                    DemoteHeading,
                    HeadingLevel1,
                    HeadingLevel2,
                    HeadingLevel3,
                    HeadingLevel4,
                    HeadingLevel5,
                    HeadingLevel6,
                    HeadingToBody,
                ]),
            ],

            Menu::View => vec![
                Standard(vec![
                    StandardItem::new("Show/Hide Toolbar", sel!(toggleToolbarShown:)),
                    StandardItem::new("Customize Toolbar…", sel!(runToolbarCustomizationPalette:)),
                    StandardItem::new("Enter Full Screen", sel!(toggleFullScreen:)).key("f").modifiers(control | command),
                ]),
                Commands(vec![SourceMode]),
                Commands(vec![ZoomLevel1, ZoomLevel2, ZoomLevel3, ZoomLevel4, ZoomLevel5, ZoomIn, ZoomOut]),
                Commands(vec![IncreaseTextSize, DecreaseTextSize, ResetTextSize]),
                Commands(vec![TaskPanel]),
                Commands(vec![CommandPalette, DocumentLens, ReaderProfiles]),
                Commands(vec![DocumentHealth, RenderTargets, VisualDebugger, ReviewPanel]),
                Commands(vec![Workspace, LocalAi]),
                Commands(vec![FocusMode, TypewriterScrolling, StatusBar]),
                DynamicSubmenu {
                    title: "Theme",
                    delegate: ProtocolObject::from_retained(ThemeMenuDelegate::shared(mtm)),
                },
                Commands(vec![ReloadTheme]),
            ],

            Menu::Navigate => vec![
                Commands(vec![PreviousHeading, NextHeading]),
                Commands(vec![PreviousLink, NextLink, FollowLinkAtCaret]),
                Commands(vec![GoToLine]),
                Commands(vec![PreviousChange, NextChange, MarkChangesReviewed]),
                Commands(vec![GoBack, GoForward]),
                Commands(vec![ScrollUp, ScrollDown, PageUp, PageDown]),
                Commands(vec![DocumentStart, DocumentEnd]),
            ],

            Menu::Document => vec![
                Commands(vec![TidyDocument]),
                Commands(vec![MoveBlockUp, MoveBlockDown]),
                Commands(vec![FoldSection, UnfoldSection, FoldAll, UnfoldAll]),
                Commands(vec![SortListAlphabetically, SortListByState, InsertTableOfContents]),
                Commands(vec![FrontMatterEditor, TableEditor, AssetDoctor]),
            ],

            Menu::Window => vec![
                Standard(vec![
                    StandardItem::new("Minimize", sel!(performMiniaturize:)).key("m"),
                    StandardItem::new("Zoom", sel!(performZoom:)),
                ]),
                Commands(vec![SplitView, PinWindow]),
                Standard(vec![
                    StandardItem::new("Show Previous Tab", sel!(selectPreviousTab:)).key("\t").modifiers(control | shift),
                    StandardItem::new("Show Next Tab", sel!(selectNextTab:)).key("\t").modifiers(control),
                    StandardItem::new("Move Tab to New Window", sel!(moveTabToNewWindow:)),
                    StandardItem::new("Merge All Windows", sel!(mergeAllWindows:)),
                    StandardItem::new("Show Tab Bar", sel!(toggleTabBar:)),
                ]),
                Standard(vec![StandardItem::new("Bring All to Front", sel!(arrangeInFront:))]),
            ],

            // Titles here are what the Help menu's search field indexes, so
            // they are written the way a user would ask for them.
            Menu::Help => vec![
                Standard(vec![
                    // The start window retires its tour button after a few
                    // launches; this is where it goes to stay reachable.
                    StandardItem::new("Take the Tour", sel!(takeTour:)),
                    StandardItem::new("Upleft Help", sel!(openLink:))
                        .key("?")
                        .target(Retained::into_super(HelpLinkTarget::shared(mtm)))
                        .represented_object("https://github.com/bitemyapp/upleft#readme"),
                    StandardItem::link("Markdown Reference", "https://commonmark.org/help/", mtm),
                ]),
                Standard(vec![
                    StandardItem::link("Report an Issue", "https://github.com/bitemyapp/upleft/issues/new", mtm),
                    StandardItem::new("Star Upleft on GitHub", sel!(openProjectPage:)),
                    StandardItem::link("Support Upleft", "https://github.com/sponsors/ezzy1630", mtm),
                ]),
            ],
        }
    }

    // MARK: - Building

    fn menu_item(menu: Menu, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&menu.title()));
        submenu.setAutoenablesItems(true);

        for group in Self::groups(menu, mtm) {
            if submenu.numberOfItems() != 0 {
                submenu.addItem(&NSMenuItem::separatorItem(mtm));
            }
            match group {
                MenuGroup::Commands(commands) => {
                    for command in commands {
                        submenu.addItem(&Self::command_item(command, mtm));
                    }
                }
                MenuGroup::Submenu { title, commands } => {
                    let items: Vec<Retained<NSMenuItem>> =
                        commands.into_iter().map(|command| Self::command_item(command, mtm)).collect();
                    submenu.addItem(&Self::host_item(title, items, mtm));
                }
                MenuGroup::Standard(items) => {
                    for standard in items {
                        submenu.addItem(&standard.make_menu_item(mtm));
                    }
                }
                MenuGroup::Services => submenu.addItem(&Self::services_item(mtm)),
                MenuGroup::ImportFromDevice => submenu.addItem(&Self::import_from_device_item(mtm)),
                MenuGroup::StandardSubmenu { title, items } => {
                    let items: Vec<Retained<NSMenuItem>> =
                        items.into_iter().map(|standard| standard.make_menu_item(mtm)).collect();
                    submenu.addItem(&Self::host_item(title, items, mtm));
                }
                MenuGroup::DynamicSubmenu { title, delegate } => {
                    let title = NSString::from_str(title);
                    let host = menu_item_with(&title, None, "", mtm);
                    let child = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
                    child.setDelegate(Some(&delegate));
                    host.setSubmenu(Some(&child));
                    submenu.addItem(&host);
                }
            }
        }

        match menu {
            Menu::Window => NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&submenu)),
            Menu::Help => NSApplication::sharedApplication(mtm).setHelpMenu(Some(&submenu)),
            _ => {}
        }

        item.setSubmenu(Some(&submenu));
        item
    }

    fn host_item(title: &str, items: Vec<Retained<NSMenuItem>>, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let title = NSString::from_str(title);
        let host = menu_item_with(&title, None, "", mtm);
        let child = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
        child.setAutoenablesItems(true);
        for item in &items {
            child.addItem(item);
        }
        host.setSubmenu(Some(&child));
        host
    }

    fn services_item(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let services = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Services"));
        let item = menu_item_with(&NSString::from_str("Services"), None, "", mtm);
        item.setSubmenu(Some(&services));
        NSApplication::sharedApplication(mtm).setServicesMenu(Some(&services));
        item
    }

    /// The Continuity Camera host, built exactly the way TextEdit's is: an
    /// "Insert" submenu holding one placeholder whose only meaningful
    /// property is `NSMenuItem.importFromDeviceIdentifier`.
    ///
    /// AppKit swaps that placeholder for "Take Photo" and "Scan Documents"
    /// when a nearby iPhone or iPad can serve the request *and* the responder
    /// chain advertises image return types — `DocumentWindowController` does,
    /// in `validRequestor(forSendType:returnType:)`. Nothing here has an
    /// action: the substituted items carry their own, and a hand-written
    /// selector would only be a second, wrong answer for the state where no
    /// device is in range. The visible title is what the user sees until one
    /// is.
    fn import_from_device_item(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let host = menu_item_with(&NSString::from_str("Insert"), None, "", mtm);
        let child = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Insert"));
        child.setAutoenablesItems(true);
        let placeholder = menu_item_with(&NSString::from_str("Import from iPhone or iPad"), None, "", mtm);
        // SAFETY: AppKit's identifier constant.
        placeholder.setIdentifier(Some(unsafe { NSMenuItemImportFromDeviceIdentifier }));
        child.addItem(&placeholder);
        host.setSubmenu(Some(&child));
        host
    }

    // MARK: - Items

    pub fn command_item(command: Command, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let item = menu_item_with(
            &NSString::from_str(command.title()),
            Some(perform_downright_command_selector()),
            "",
            mtm,
        );
        // SAFETY: an `NSString` is a valid represented object.
        unsafe { item.setRepresentedObject(Some(&NSString::from_str(command.raw_value()))) };
        item.setTag(Self::command_tag(command));
        Self::apply_key_equivalent(&item, command);
        item
    }

    pub fn command_tag(command: Command) -> isize {
        Command::ALL_CASES.iter().position(|candidate| *candidate == command).unwrap_or(0) as isize + 1000
    }

    /// `command(for:)`: the command a menu item carries in its
    /// `representedObject`, if any.
    pub fn command(item: &NSMenuItem) -> Option<Command> {
        let represented = item.representedObject()?;
        let string = represented.downcast::<NSString>().ok()?;
        Command::from_raw_value(&string.to_string())
    }

    /// Menu validation, derived from the command table's preconditions.
    ///
    /// Both `AppDelegate` and `DocumentWindowController` route their
    /// `NSMenuItemValidation` here, so "is Save available?" has one answer.
    /// Items that carry no command are left to whoever owns them.
    pub fn validate(item: &NSMenuItem, context: &CommandContext) -> bool {
        let Some(command) = Self::command(item) else { return true };
        command.is_enabled(context)
    }

    /// Rebuilds shortcut display after the user remaps a binding.
    pub fn refresh_key_equivalents(menu: &NSMenu) {
        for item in menu.itemArray().iter() {
            if let Some(command) = Self::command(&item) {
                Self::apply_key_equivalent(&item, command);
            }
            if let Some(submenu) = item.submenu() {
                Self::refresh_key_equivalents(&submenu);
            }
        }
    }

    fn apply_key_equivalent(item: &NSMenuItem, command: Command) {
        // Only ⌘/⌃ bindings become menu key equivalents: a bare `n` in the
        // menu bar would fire while the user is typing in Live mode.
        let binding = KeybindingStore::shared().primary_binding(command);
        let Some(binding) = binding.filter(|binding| {
            binding.modifiers.contains(ModifierFlags::COMMAND) || binding.modifiers.contains(ModifierFlags::CONTROL)
        }) else {
            item.setKeyEquivalent(&NSString::from_str(""));
            item.setKeyEquivalentModifierMask(NSEventModifierFlags(0));
            return;
        };
        item.setKeyEquivalent(&NSString::from_str(&binding.menu_key_equivalent()));
        item.setKeyEquivalentModifierMask(event_flags(binding.modifiers));
    }
}

/// `NSEvent.ModifierFlags` from the command table's copy of AppKit's bits.
fn event_flags(flags: ModifierFlags) -> NSEventModifierFlags {
    NSEventModifierFlags(flags.raw_value() as usize)
}

/// `NSMenuItem(title:action:keyEquivalent:)`.
fn menu_item_with(title: &NSString, action: Option<Sel>, key_equivalent: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    // SAFETY: the action is dispatched through the responder chain (or to the
    // explicit target), which answers it with the Swift signature `(Any?)`.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            title,
            action,
            &NSString::from_str(key_equivalent),
        )
    }
}

// MARK: - Menu description

/// One separated run inside a menu.
///
/// Commands come from the table. "Standard" runs are AppKit actions the
/// responder chain already implements — Cut, Paste and Match Style,
/// Spelling, Enter Full Screen — which the command table deliberately does
/// not duplicate, because they are not ours to rebind.
enum MenuGroup {
    Commands(Vec<Command>),
    Submenu { title: &'static str, commands: Vec<Command> },
    Standard(Vec<StandardItem>),
    StandardSubmenu { title: &'static str, items: Vec<StandardItem> },
    /// A submenu whose contents a delegate supplies when it opens.
    DynamicSubmenu { title: &'static str, delegate: Retained<ProtocolObject<dyn NSMenuDelegate>> },
    /// The one submenu macOS fills in for us.
    Services,
    /// The Continuity Camera submenu macOS fills in for us — Take Photo and
    /// Scan Documents, served by a nearby iPhone or iPad.
    ImportFromDevice,
}

/// A menu item for an action the app does not own.
struct StandardItem {
    title: &'static str,
    selector: Option<Sel>,
    key_equivalent: &'static str,
    modifiers: ModifierFlags,
    target: Option<Retained<NSObject>>,
    represented_object: Option<&'static str>,
}

impl StandardItem {
    /// `StandardItem(title:selector:)` with the defaults (`keyEquivalent:
    /// ""`, `modifiers: .command`, no target, no represented object).
    fn new(title: &'static str, selector: Sel) -> StandardItem {
        StandardItem {
            title,
            selector: Some(selector),
            key_equivalent: "",
            modifiers: ModifierFlags::COMMAND,
            target: None,
            represented_object: None,
        }
    }

    /// `StandardItem.separator`.
    fn separator() -> StandardItem {
        StandardItem {
            title: "-",
            selector: None,
            key_equivalent: "",
            modifiers: ModifierFlags::COMMAND,
            target: None,
            represented_object: None,
        }
    }

    fn key(mut self, key_equivalent: &'static str) -> StandardItem {
        self.key_equivalent = key_equivalent;
        self
    }

    fn modifiers(mut self, modifiers: ModifierFlags) -> StandardItem {
        self.modifiers = modifiers;
        self
    }

    fn target(mut self, target: Retained<NSObject>) -> StandardItem {
        self.target = Some(target);
        self
    }

    fn represented_object(mut self, represented_object: &'static str) -> StandardItem {
        self.represented_object = Some(represented_object);
        self
    }

    fn make_menu_item(&self, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        let Some(selector) = self.selector else { return NSMenuItem::separatorItem(mtm) };
        let item = menu_item_with(&NSString::from_str(self.title), Some(selector), self.key_equivalent, mtm);
        if !self.key_equivalent.is_empty() {
            item.setKeyEquivalentModifierMask(event_flags(self.modifiers));
        }
        let represented = self.represented_object.map(NSString::from_str);
        // SAFETY: the target answers the selector (or is nil: the responder
        // chain), and an `NSString` is a valid represented object.
        unsafe {
            item.setTarget(self.target.as_deref().map(|target| &**target));
            item.setRepresentedObject(represented.as_deref().map(|string| string.as_ref()));
        }
        item
    }

    /// `StandardItem.link(_:_:)`.
    fn link(title: &'static str, url: &'static str, mtm: MainThreadMarker) -> StandardItem {
        StandardItem::new(title, sel!(openLink:))
            .target(Retained::into_super(HelpLinkTarget::shared(mtm)))
            .represented_object(url)
    }
}

// MARK: - CommandResponder

/// `@objc protocol CommandResponder`: any object in the responder chain that
/// can run a `Command`.
///
/// Menu items dispatch by selector, so what makes a `define_class!` type a
/// command responder is the Objective-C method `performDownrightCommand:`
/// (register it with `#[unsafe(method(performDownrightCommand:))]` and
/// forward to this trait). No code asks the runtime for the protocol itself.
pub trait CommandResponder {
    /// `@objc func performDownrightCommand(_ sender: Any?)`.
    fn perform_downright_command(&self, sender: Option<&AnyObject>);
}

/// `#selector(CommandResponder.performDownrightCommand(_:))`.
pub fn perform_downright_command_selector() -> Sel {
    sel!(performDownrightCommand:)
}

// MARK: - Update check revalidation

pub struct UpdateCheckMenuItemIvars {
    state_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Drop for UpdateCheckMenuItemIvars {
    /// `deinit`.
    fn drop(&mut self) {
        if let Some(state_observer) = self.state_observer.get_mut().take() {
            // SAFETY: removes the block observer this object registered.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(state_observer.as_ref()) };
        }
    }
}

define_class!(
    /// Keeps "Check for Updates…" honest. The item itself is an ordinary
    /// command item now; this only forces a menu revalidation pass whenever
    /// the updater's state changes, so `canCheckForUpdates` (KVO-backed by
    /// Sparkle) reaches the menu without polling.
    // SAFETY: `init` is forwarded in `shared` after the ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateCheckMenuItem"]
    #[ivars = UpdateCheckMenuItemIvars]
    pub struct UpdateCheckMenuItem;

    unsafe impl NSObjectProtocol for UpdateCheckMenuItem {}
);

thread_local! {
    static UPDATE_CHECK_MENU_ITEM: OnceCell<Retained<UpdateCheckMenuItem>> = const { OnceCell::new() };
    static HELP_LINK_TARGET: OnceCell<Retained<HelpLinkTarget>> = const { OnceCell::new() };
    static RECENTS_MENU_DELEGATE: OnceCell<Retained<RecentsMenuDelegate>> = const { OnceCell::new() };
    static THEME_MENU_DELEGATE: OnceCell<Retained<ThemeMenuDelegate>> = const { OnceCell::new() };
}

impl UpdateCheckMenuItem {
    /// `UpdateCheckMenuItem.shared`.
    pub fn shared(mtm: MainThreadMarker) -> Retained<UpdateCheckMenuItem> {
        UPDATE_CHECK_MENU_ITEM.with(|cell| cell.get_or_init(|| Self::init(mtm)).clone())
    }

    /// `private override init()`.
    fn init(mtm: MainThreadMarker) -> Retained<UpdateCheckMenuItem> {
        let this = Self::alloc(mtm).set_ivars(UpdateCheckMenuItemIvars { state_observer: RefCell::new(None) });
        let this: Retained<UpdateCheckMenuItem> = unsafe { msg_send![super(this), init] };
        let weak_self: ObjcWeak<UpdateCheckMenuItem> = ObjcWeak::from(&*this);
        let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
            if let Some(this) = weak_self.load() {
                this.refresh_menu();
            }
        });
        // SAFETY: the block runs on the main queue (`queue: .main`), where
        // the weak reference is loaded.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str(UpdateCoordinator::STATE_DID_CHANGE)),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *this.ivars().state_observer.borrow_mut() = Some(observer);
        this
    }

    fn refresh_menu(&self) {
        if let Some(main_menu) = NSApplication::sharedApplication(self.mtm()).mainMenu() {
            main_menu.update();
        }
    }
}

// MARK: - Help links

define_class!(
    /// Target for the Help menu's documentation links. A menu item's URL
    /// lives in its `representedObject`, so adding a link is one line in
    /// `groups(in:)`.
    // SAFETY: `init` is inherited from `NSObject`; no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "HelpLinkTarget"]
    pub struct HelpLinkTarget;

    unsafe impl NSObjectProtocol for HelpLinkTarget {}

    impl HelpLinkTarget {
        #[unsafe(method(openLink:))]
        fn open_link(&self, sender: &NSMenuItem) {
            let Some(string) = sender.representedObject().and_then(|object| object.downcast::<NSString>().ok()) else {
                return;
            };
            let Some(url) = NSURL::URLWithString(&string) else { return };
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
);

impl HelpLinkTarget {
    /// `HelpLinkTarget.shared`.
    pub fn shared(mtm: MainThreadMarker) -> Retained<HelpLinkTarget> {
        HELP_LINK_TARGET.with(|cell| cell.get_or_init(|| unsafe { msg_send![Self::alloc(mtm), init] }).clone())
    }
}

// MARK: - Dynamic submenus

define_class!(
    // SAFETY: `init` is inherited from `NSObject`; no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RecentsMenuDelegate"]
    pub struct RecentsMenuDelegate;

    unsafe impl NSObjectProtocol for RecentsMenuDelegate {}

    unsafe impl NSMenuDelegate for RecentsMenuDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let mtm = self.mtm();
            menu.removeAllItems();
            let recents = DocumentStateStore::shared().recents(15);
            if recents.is_empty() {
                let empty = menu_item_with(&NSString::from_str("No Recent Documents"), None, "", mtm);
                empty.setEnabled(false);
                menu.addItem(&empty);
                return;
            }
            for recent in &recents {
                let item = menu_item_with(&NSString::from_str(&recent.display_name), Some(sel!(openRecentDocument:)), "", mtm);
                // SAFETY: an `NSString` is a valid represented object.
                unsafe { item.setRepresentedObject(Some(&NSString::from_str(&recent.path))) };
                // The first heading is a far better identifier than the
                // filename for agent output, which is full of `output.md` and
                // `plan.md`.
                if !recent.first_heading.is_empty() {
                    item.setToolTip(Some(&NSString::from_str(&recent.first_heading)));
                }
                menu.addItem(&item);
            }
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&menu_item_with(&NSString::from_str("Clear Menu"), Some(sel!(clearRecentDocuments:)), "", mtm));
        }
    }
);

impl RecentsMenuDelegate {
    /// `RecentsMenuDelegate.shared`.
    pub fn shared(mtm: MainThreadMarker) -> Retained<RecentsMenuDelegate> {
        RECENTS_MENU_DELEGATE.with(|cell| cell.get_or_init(|| unsafe { msg_send![Self::alloc(mtm), init] }).clone())
    }
}

define_class!(
    // SAFETY: `init` is inherited from `NSObject`; no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ThemeMenuDelegate"]
    pub struct ThemeMenuDelegate;

    unsafe impl NSObjectProtocol for ThemeMenuDelegate {}

    unsafe impl NSMenuDelegate for ThemeMenuDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let mtm = self.mtm();
            menu.removeAllItems();

            let follow = menu_item_with(
                &NSString::from_str("Follow macOS Appearance"),
                Some(sel!(toggleFollowSystemAppearance:)),
                "",
                mtm,
            );
            follow.setState(if Preferences::shared().values().follows_system_appearance {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            menu.addItem(&follow);
            menu.addItem(&NSMenuItem::separatorItem(mtm));

            menu.addItem(&Self::theme_picker(
                "Light Theme",
                ThemeStore::shared().themes().into_iter().filter(|theme| theme.appearance != ThemeAppearance::Dark).collect(),
                &Preferences::shared().values().theme_name,
                sel!(selectLightTheme:),
                mtm,
            ));
            menu.addItem(&Self::theme_picker(
                "Dark Theme",
                ThemeStore::shared().themes().into_iter().filter(|theme| theme.appearance != ThemeAppearance::Light).collect(),
                &Preferences::shared().values().dark_theme_name,
                sel!(selectDarkTheme:),
                mtm,
            ));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&menu_item_with(&NSString::from_str("Import VS Code Theme…"), Some(sel!(importTheme:)), "", mtm));
            menu.addItem(&menu_item_with(&NSString::from_str("Reveal Themes Folder"), Some(sel!(revealThemesFolder:)), "", mtm));
        }
    }
);

impl ThemeMenuDelegate {
    /// `ThemeMenuDelegate.shared`.
    pub fn shared(mtm: MainThreadMarker) -> Retained<ThemeMenuDelegate> {
        THEME_MENU_DELEGATE.with(|cell| cell.get_or_init(|| unsafe { msg_send![Self::alloc(mtm), init] }).clone())
    }

    fn theme_picker(
        title: &str,
        themes: Vec<Theme>,
        selected_name: &str,
        action: Sel,
        mtm: MainThreadMarker,
    ) -> Retained<NSMenuItem> {
        let title = NSString::from_str(title);
        let parent = menu_item_with(&title, None, "", mtm);
        let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
        for theme in &themes {
            let item = menu_item_with(&NSString::from_str(&theme.name), Some(action), "", mtm);
            // SAFETY: an `NSString` is a valid represented object.
            unsafe { item.setRepresentedObject(Some(&NSString::from_str(&theme.name))) };
            item.setState(if swift_text::str_eq(&theme.name, selected_name) {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            submenu.addItem(&item);
        }
        parent.setSubmenu(Some(&submenu));
        parent
    }
}
