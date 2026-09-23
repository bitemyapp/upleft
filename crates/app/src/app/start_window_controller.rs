//! Port of `App/StartWindowController.swift`: the start window. One task
//! path — brand, one sentence, Open and New (and the tour), then the recent
//! files — in a fixed, compact window that hands off to a document window.
//!
//! Every Swift class keeps its unqualified name as its Objective-C class
//! name, private ones included, so the window's view dump compares:
//!
//! | Swift | Rust | Objective-C class |
//! |---|---|---|
//! | `startRecentDisplayLimit`, `StartLayout`, `KeycapFormatter`, `StartTheme`, `StartGuideOffer`, `StartWindowController`, `configurePassiveLabel` | this file | `StartWindowController` |
//! | `StartView`, `StartDropOverlay`, `StartCanvasView` | `start_view` | `StartView`, `StartDropOverlay`, `StartCanvasView` |
//! | `StartHeroView`, `KeycapBadgeField`, `StartActionButton`, `BrandMarkView` | `hero` | `StartHeroView`, `KeycapBadgeField`, `StartActionButton`, `BrandMarkView` |
//! | `RecentDocumentsPanel`, `RecentEmptyState`, `RecentDocumentButton` | `recents` | `RecentDocumentsPanel`, `RecentEmptyState`, `RecentDocumentButton` |
//! | `RecentRowCopy` | `recent_row_copy` | (a Swift `enum`) |
//!
//! The update pill (`UpdateStatusPill(presentation: .compactWarning)`) is
//! ported on `port/panels`; until it lands, `start_view::install_update_pill`
//! (marked `// PANELS: UpdateStatusPill`) holds its code and does nothing.

mod hero;
mod recent_row_copy;
mod recents;
mod start_view;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAppearance, NSApplication, NSBackingStoreType, NSButton, NSColor, NSFont,
    NSFontWeightSemibold, NSMenuItem, NSMutableParagraphStyle, NSPasteboard, NSPasteboardTypeString, NSResponder,
    NSTextAlignment, NSTextField, NSUserInterfaceItemIdentification, NSWindow, NSWindowController,
    NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{
    NSArray, NSAttributedString, NSMutableAttributedString, NSNumber, NSPoint, NSRect, NSSize, NSString,
};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{attributed_string, keys, main_async, ns_string};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use crate::ai::document_state_store::RecentDocument;

pub use recent_row_copy::RecentRowCopy;
use start_view::StartView;

/// `startRecentDisplayLimit`: six rows fit the compact start window without
/// a scrolling container.
const START_RECENT_DISPLAY_LIMIT: usize = 6;

/// `StartLayout`.
///
/// Internal rather than private so the start window's tests can assert
/// against the constants instead of copies of them: the one that pinned a
/// literal 576 broke the moment a recent row grew a second line, which told
/// nobody anything about the window being fixed-size, which is what it
/// meant to check.
pub struct StartLayout;

impl StartLayout {
    // A fixed, well-proportioned welcome surface. The 556pt content column
    // maps cleanly to the 2x reference capture while leaving a quiet 28pt
    // window margin on either side.
    pub const WINDOW_SIZE: NSSize = NSSize::new(612.0, 560.0);
    pub const HORIZONTAL_INSET: CGFloat = 28.0;
    pub const TOP_INSET: CGFloat = 42.0;
    pub const BOTTOM_INSET: CGFloat = 24.0;
    pub const SECTION_SPACING: CGFloat = 18.0;
    pub const CONTENT_WIDTH: CGFloat = 556.0;
    pub const BUTTON_HEIGHT: CGFloat = 40.0;
    pub const ACTION_SPACING: CGFloat = 10.0;
    // Two peer actions span the 556pt content column with balanced 273pt wells:
    pub const ACTION_BUTTON_WIDTH: CGFloat = 273.0;
    pub const ROW_HEIGHT: CGFloat = 44.0;
    pub const ROW_SPACING: CGFloat = 2.0;
    pub const CORNER_RADIUS: CGFloat = 7.0;

    /// `populatedListHeight`: what a full recents list measures. The empty
    /// state matches it so the window does not change weight between having
    /// files and not.
    pub fn populated_list_height() -> CGFloat {
        let rows = START_RECENT_DISPLAY_LIMIT as CGFloat;
        rows * Self::ROW_HEIGHT + (rows - 1.0) * Self::ROW_SPACING
    }
}

/// `KeycapFormatter`: optical keycap formatter that aligns modifier symbols
/// (like ⌘) with key characters (like O, N, 1..9) using subtle tracking and
/// baseline adjustments.
pub struct KeycapFormatter;

impl KeycapFormatter {
    /// `format(shortcut:color:)`.
    pub fn format(shortcut: &str, color: &NSColor) -> Retained<NSAttributedString> {
        let result = NSMutableAttributedString::new();
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setAlignment(NSTextAlignment::Center);

        // SAFETY: AppKit's attribute-name and font-weight constants.
        let (baseline_offset, kern, semibold) = unsafe {
            (objc2_app_kit::NSBaselineOffsetAttributeName, objc2_app_kit::NSKernAttributeName, NSFontWeightSemibold)
        };
        let count = upleft_swift_text::count(shortcut) as isize;
        for (index, character) in upleft_swift_text::graphemes(shortcut).enumerate() {
            let is_last = index as isize == count - 1;
            let zero = NSNumber::new_f64(0.0);
            let piece = if ["⌘", "⇧", "⌥", "⌃"].iter().any(|symbol| upleft_swift_text::char_eq(character, symbol)) {
                let font = NSFont::systemFontOfSize_weight(11.5, semibold);
                let tracking = NSNumber::new_f64(if is_last { 0.0 } else { 1.2 });
                attributed_string(
                    character,
                    &[
                        (keys::font(), &font),
                        (keys::foreground_color(), color),
                        (keys::paragraph_style(), &paragraph),
                        (baseline_offset, &zero),
                        (kern, &tracking),
                    ],
                )
            } else {
                let font = if upleft_swift_text::is_number(character) {
                    NSFont::monospacedDigitSystemFontOfSize_weight(11.5, semibold)
                } else {
                    NSFont::systemFontOfSize_weight(11.5, semibold)
                };
                attributed_string(
                    character,
                    &[
                        (keys::font(), &font),
                        (keys::foreground_color(), color),
                        (keys::paragraph_style(), &paragraph),
                        (baseline_offset, &zero),
                    ],
                )
            };
            result.appendAttributedString(&piece);
        }
        Retained::into_super(result)
    }
}

/// `StartTheme`: the start window draws from the app's selected theme so the
/// welcome surface agrees with the editor it hands off to. One factory,
/// three users.
struct StartTheme;

impl StartTheme {
    /// `makeSheet(appearance:)`. Pass the view's own appearance once it is
    /// in a window: the start window pins itself to the theme, so `NSApp`'s
    /// answer is only right before that lands.
    fn make_sheet(appearance: Option<&NSAppearance>, mtm: MainThreadMarker) -> StyleSheet {
        let theme = ThemeStore::shared().current();
        match appearance {
            Some(appearance) => StyleSheet::new(theme, appearance, None),
            None => StyleSheet::new(theme, &NSApplication::sharedApplication(mtm).effectiveAppearance(), None),
        }
    }
}

/// `StartGuideOffer`: where the welcome document sits on the start window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StartGuideOffer {
    /// This build ships no welcome document; the action is not shown.
    #[default]
    Unavailable,
    /// A quiet third action beside Open and New.
    Secondary,
    /// First launch: the guide leads, because there is nothing to reopen and
    /// nothing to continue.
    Primary,
}

/// `onOpen`.
pub type OpenHandler = Rc<dyn Fn(FileUrl)>;
/// `onOpenPanel`, `onNew`, `onOpenGuide`, `onClearRecents`.
pub type ActionHandler = Rc<dyn Fn()>;
/// `onRemoveRecent`.
pub type RemoveRecentHandler = Rc<dyn Fn(String)>;

pub struct StartWindowControllerIvars {
    on_open: RefCell<Option<OpenHandler>>,
    on_open_panel: RefCell<Option<ActionHandler>>,
    on_new: RefCell<Option<ActionHandler>>,
    on_open_guide: RefCell<Option<ActionHandler>>,
    on_clear_recents: RefCell<Option<ActionHandler>>,
    on_remove_recent: RefCell<Option<RemoveRecentHandler>>,
    is_handing_off: Cell<bool>,
}

define_class!(
    // SAFETY: `initWithWindow:` is forwarded in `new` after the ivars are
    // set; the action methods keep AppKit's `-action:` signature.
    #[unsafe(super(NSWindowController, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "StartWindowController"]
    #[ivars = StartWindowControllerIvars]
    pub struct StartWindowController;

    unsafe impl NSObjectProtocol for StartWindowController {}

    impl StartWindowController {
        /// Quiet text-button target for the recents header's Clear action.
        #[unsafe(method(clearRecents:))]
        fn __clear_recents(&self, sender: Option<&AnyObject>) {
            self.clear_recents(sender);
        }

        // Row actions carry their path on the menu item, so one menu shape
        // serves every row and nothing has to ask which one was clicked.

        #[unsafe(method(showRecentInFinder:))]
        fn __show_recent_in_finder(&self, sender: &NSMenuItem) {
            self.show_recent_in_finder(sender);
        }

        #[unsafe(method(copyRecentPath:))]
        fn __copy_recent_path(&self, sender: &NSMenuItem) {
            self.copy_recent_path(sender);
        }

        #[unsafe(method(removeRecent:))]
        fn __remove_recent(&self, sender: &NSMenuItem) {
            self.remove_recent(sender);
        }

        #[unsafe(method(openRecent:))]
        fn __open_recent(&self, sender: &NSButton) {
            self.open_recent(sender);
        }

        #[unsafe(method(openPanel:))]
        fn __open_panel(&self, sender: Option<&AnyObject>) {
            self.open_panel(sender);
        }

        #[unsafe(method(newDocument:))]
        fn __new_document(&self, sender: Option<&AnyObject>) {
            self.new_document(sender);
        }

        #[unsafe(method(openGuide:))]
        fn __open_guide(&self, sender: Option<&AnyObject>) {
            self.open_guide(sender);
        }
    }
);

impl StartWindowController {
    /// `recentDisplayLimit`.
    pub const RECENT_DISPLAY_LIMIT: usize = START_RECENT_DISPLAY_LIMIT;

    /// `convenience init(recents:guide:)` (Swift's default guide is
    /// [`StartGuideOffer::Unavailable`]).
    pub fn new(recents: Vec<RecentDocument>, guide: StartGuideOffer, mtm: MainThreadMarker) -> Retained<StartWindowController> {
        // SAFETY: a plain titled window; its controller owns it (AppKit
        // ignores `releasedWhenClosed` for windows owned by a controller).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), StartLayout::WINDOW_SIZE),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::FullSizeContentView,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("Upleft"));
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        window.setMovableByWindowBackground(true);
        window.setMinSize(StartLayout::WINDOW_SIZE);
        window.setMaxSize(StartLayout::WINDOW_SIZE);
        window.setRestorable(false);
        window.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        window.center();
        let this = Self::alloc(mtm).set_ivars(StartWindowControllerIvars {
            on_open: RefCell::new(None),
            on_open_panel: RefCell::new(None),
            on_new: RefCell::new(None),
            on_open_guide: RefCell::new(None),
            on_clear_recents: RefCell::new(None),
            on_remove_recent: RefCell::new(None),
            is_handing_off: Cell::new(false),
        });
        let this: Retained<StartWindowController> = unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };
        let content = StartView::new(&recents, guide, &this, mtm);
        window.setContentView(Some(&content));
        window.setInitialFirstResponder(Some(&content.preferred_first_responder()));
        this
    }

    // MARK: Callbacks

    pub fn on_open(&self) -> Option<OpenHandler> {
        self.ivars().on_open.borrow().clone()
    }

    pub fn set_on_open(&self, handler: Option<OpenHandler>) {
        *self.ivars().on_open.borrow_mut() = handler;
    }

    pub fn on_open_panel(&self) -> Option<ActionHandler> {
        self.ivars().on_open_panel.borrow().clone()
    }

    pub fn set_on_open_panel(&self, handler: Option<ActionHandler>) {
        *self.ivars().on_open_panel.borrow_mut() = handler;
    }

    pub fn on_new(&self) -> Option<ActionHandler> {
        self.ivars().on_new.borrow().clone()
    }

    pub fn set_on_new(&self, handler: Option<ActionHandler>) {
        *self.ivars().on_new.borrow_mut() = handler;
    }

    pub fn on_open_guide(&self) -> Option<ActionHandler> {
        self.ivars().on_open_guide.borrow().clone()
    }

    pub fn set_on_open_guide(&self, handler: Option<ActionHandler>) {
        *self.ivars().on_open_guide.borrow_mut() = handler;
    }

    pub fn on_clear_recents(&self) -> Option<ActionHandler> {
        self.ivars().on_clear_recents.borrow().clone()
    }

    pub fn set_on_clear_recents(&self, handler: Option<ActionHandler>) {
        *self.ivars().on_clear_recents.borrow_mut() = handler;
    }

    pub fn on_remove_recent(&self) -> Option<RemoveRecentHandler> {
        self.ivars().on_remove_recent.borrow().clone()
    }

    pub fn set_on_remove_recent(&self, handler: Option<RemoveRecentHandler>) {
        *self.ivars().on_remove_recent.borrow_mut() = handler;
    }

    /// `onOpen?(url)`.
    fn call_on_open(&self, url: FileUrl) {
        if let Some(handler) = self.on_open() {
            handler(url);
        }
    }

    fn call(handler: Option<ActionHandler>) {
        if let Some(handler) = handler {
            handler();
        }
    }

    // MARK: Actions

    /// `clearRecents(_:)`.
    pub fn clear_recents(&self, _sender: Option<&AnyObject>) {
        Self::call(self.on_clear_recents());
    }

    /// `showRecentInFinder(_:)`.
    pub fn show_recent_in_finder(&self, sender: &NSMenuItem) {
        let Some(path) = represented_path(sender) else { return };
        let url = FileUrl::from_path(&path).to_nsurl();
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    }

    /// `copyRecentPath(_:)`.
    pub fn copy_recent_path(&self, sender: &NSMenuItem) {
        let Some(path) = represented_path(sender) else { return };
        NSPasteboard::generalPasteboard().clearContents();
        // SAFETY: AppKit's pasteboard type constant.
        NSPasteboard::generalPasteboard().setString_forType(&ns_string(&path), unsafe { NSPasteboardTypeString });
    }

    /// `removeRecent(_:)`.
    pub fn remove_recent(&self, sender: &NSMenuItem) {
        let Some(path) = represented_path(sender) else { return };
        if let Some(handler) = self.on_remove_recent() {
            handler(path);
        }
    }

    /// `startView`: `window?.contentView as? StartView`.
    fn start_view(&self) -> Option<Retained<StartView>> {
        self.window()?.contentView()?.downcast::<StartView>().ok()
    }

    /// `reloadRecents(_:)`.
    pub fn reload_recents(&self, recents: &[RecentDocument]) {
        self.ivars().is_handing_off.set(false);
        if let Some(view) = self.start_view() {
            let shown = &recents[..recents.len().min(Self::RECENT_DISPLAY_LIMIT)];
            view.reload_recents(shown);
        }
    }

    /// `dismiss(animated:completion:)`.
    pub fn dismiss(&self, animated: bool, completion: Option<Box<dyn FnOnce()>>) {
        let Some(window) = self.window() else {
            if let Some(completion) = completion {
                completion();
            }
            return;
        };
        let completion = Rc::new(RefCell::new(completion));
        let finish = {
            let window = window.clone();
            move || {
                window.setAlphaValue(1.0);
                window.close();
                let completion = completion.borrow_mut().take();
                if let Some(completion) = completion {
                    completion();
                }
            }
        };
        if !animated || StartTheme::make_sheet(None, self.mtm()).reduce_motion {
            finish();
            return;
        }
        motion::run(
            false,
            motion::QUICK,
            Curve::EaseOut,
            move |_| window.animator().setAlphaValue(0.0),
            Some(Box::new(finish)),
        );
    }

    /// `beginHandoff()`.
    fn begin_handoff(&self) -> bool {
        if self.ivars().is_handing_off.get() {
            return false;
        }
        self.ivars().is_handing_off.set(true);
        true
    }

    fn set_handing_off(&self, value: bool) {
        self.ivars().is_handing_off.set(value);
    }

    /// `DispatchQueue.main.async { [weak self] in guard let self,
    /// self.window?.isVisible == true else { return }; self.isHandingOff =
    /// false }`.
    fn unlock_if_still_visible(&self) {
        let weak = objc2::rc::Weak::from(self);
        main_async(move || {
            let Some(this) = weak.load() else { return };
            if this.window().is_some_and(|window| window.isVisible()) {
                this.set_handing_off(false);
            }
        });
    }

    /// `openRecent(_:)`.
    pub fn open_recent(&self, sender: &NSButton) {
        if !self.begin_handoff() {
            return;
        }
        let path = sender
            .identifier()
            .map(|identifier| upleft_swift_text::ns::foundation::to_string(&identifier))
            .or_else(|| sender.downcast_ref::<recents::RecentDocumentButton>().map(|row| row.document_path().to_owned()));
        let Some(path) = path.filter(|path| !path.is_empty()) else {
            self.set_handing_off(false);
            return;
        };
        self.call_on_open(FileUrl::from_path(&path));
        // Failed opens leave this window visible — unlock so the user can retry.
        self.unlock_if_still_visible();
    }

    /// `openPanel(_:)`.
    pub fn open_panel(&self, _sender: Option<&AnyObject>) {
        if !self.begin_handoff() {
            return;
        }
        Self::call(self.on_open_panel());
        self.set_handing_off(false);
    }

    /// `newDocument(_:)`.
    pub fn new_document(&self, _sender: Option<&AnyObject>) {
        if !self.begin_handoff() {
            return;
        }
        Self::call(self.on_new());
        self.set_handing_off(false);
    }

    /// `openGuide(_:)`.
    pub fn open_guide(&self, _sender: Option<&AnyObject>) {
        if !self.begin_handoff() {
            return;
        }
        Self::call(self.on_open_guide());
        self.set_handing_off(false);
    }
}

/// `sender.representedObject as? String`.
fn represented_path(item: &NSMenuItem) -> Option<String> {
    let object = item.representedObject()?;
    let string = object.downcast::<NSString>().ok()?;
    Some(upleft_swift_text::ns::foundation::to_string(&string))
}

/// `configurePassiveLabel(_:)`.
fn configure_passive_label(field: &NSTextField) {
    field.setEditable(false);
    field.setSelectable(false);
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setRefusesFirstResponder(true);
}
