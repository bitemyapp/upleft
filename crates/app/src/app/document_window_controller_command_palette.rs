//! Port of `App/DocumentWindowController+CommandPalette.swift`: floating
//! command palette presentation. The palette asks this controller to run one
//! `Command`; it never edits the document or dispatches actions itself.
//!
//! Also here: `CommandPaletteWindowDelegate` (Objective-C name as Swift's),
//! the palette panel's `NSWindowDelegate`, kept alive as an associated object
//! of the panel exactly as Swift keeps it.
//!
//! `CommandPaletteViewDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.

use std::ffi::c_void;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, ffi, msg_send};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSFloatingWindowLevel, NSPanel, NSResponder, NSSearchField, NSView,
    NSWindowButton, NSWindowDelegate, NSWindowOrderingMode, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSNotification, NSPoint};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{RectExt, rect};
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use crate::ai::document_state_store::DocumentStateStore;
use crate::app::app_delegate::{AppDelegate, DocumentOpenDisposition};
use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::panels::appkit_support::downcast;
use crate::panels::command_palette_view::{CommandPaletteView, CommandPaletteViewDelegate};
use crate::support::command_palette_model::{
    CommandPaletteModel, CommandPaletteRecentStore, UserDefaultsCommandPaletteRecentStore,
};
use crate::support::commands::Command;
use crate::support::quick_open_providers::{
    CurrentDocumentQuickOpenProvider, QuickOpenAction, QuickOpenProvider, QuickOpenResult, RecentFilesQuickOpenProvider,
};

/// `commandPaletteAssociationKey`: the palette panel's associated delegate.
static COMMAND_PALETTE_ASSOCIATION_KEY: u8 = 0;

/// The extension's associated-object state (`commandPaletteWindow`,
/// `quickOpenProviders`), held by the controller as
/// `command_palette_state()`.
#[derive(Default)]
pub struct CommandPaletteState {
    /// `commandPaletteWindow`.
    pub(crate) command_palette_window: Option<Retained<NSPanel>>,
    /// `quickOpenProviders` (`[]` until set).
    pub(crate) quick_open_providers: Vec<Rc<dyn QuickOpenProvider>>,
}

/// `NSApp.delegate as? AppDelegate`.
pub(crate) fn app_delegate(mtm: MainThreadMarker) -> Option<Retained<AppDelegate>> {
    let delegate = NSApplication::sharedApplication(mtm).delegate()?;
    let object: &AnyObject = (*delegate).as_ref();
    downcast::<AppDelegate>(object)
}

impl DocumentWindowController {
    /// `quickOpenProviders`.
    fn quick_open_providers(&self) -> Vec<Rc<dyn QuickOpenProvider>> {
        self.command_palette_state().borrow().quick_open_providers.clone()
    }

    /// `setQuickOpenProviders(_:)`.
    pub fn set_quick_open_providers(&self, providers: Vec<Rc<dyn QuickOpenProvider>>) {
        self.command_palette_state().borrow_mut().quick_open_providers = providers;
    }

    /// `commandPaletteWindow`.
    fn command_palette_window(&self) -> Option<Retained<NSPanel>> {
        self.command_palette_state().borrow().command_palette_window.clone()
    }

    fn set_command_palette_window(&self, window: Option<Retained<NSPanel>>) {
        self.command_palette_state().borrow_mut().command_palette_window = window;
    }

    /// `showCommandPalette()`: show or focus the palette.
    pub fn show_command_palette(&self) {
        if let Some(existing) = self.command_palette_window() {
            existing.makeKeyAndOrderFront(None);
            if let Some(content_view) = existing.contentView()
                && let Some(window) = content_view.window()
            {
                let field: Option<Retained<NSView>> = existing.contentView().and_then(|content| {
                    content.subviews().iter().find(|view| view.isKindOfClass(NSSearchField::class()))
                });
                match &field {
                    Some(field) => {
                        let responder: &NSResponder = field;
                        window.makeFirstResponder(Some(responder))
                    }
                    None => window.makeFirstResponder(None),
                };
            }
            return;
        }

        let mtm = MainThreadMarker::from(self);
        // Swift's argument order: the style sheet, the recent store, then the
        // model (commands, recents, providers), then the providers again.
        let style_sheet = self.active_style_sheet();
        let recent_store: Rc<dyn CommandPaletteRecentStore> =
            Rc::new(UserDefaultsCommandPaletteRecentStore::standard());
        // `commandContext` is a computed property Swift's filter closure reads
        // once per command; it has no side effects, so it is read once here.
        let context = self.command_context();
        let commands: Vec<Command> =
            Command::ALL_CASES.iter().copied().filter(|command| command.is_enabled(&context)).collect();
        let model = CommandPaletteModel::with_commands(
            &commands,
            CommandPaletteModel::store_bindings,
            UserDefaultsCommandPaletteRecentStore::standard().recent_commands(),
            self.palette_providers(),
        );
        let providers = self.palette_providers();
        let palette = CommandPaletteView::new(style_sheet, recent_store, Some(model), providers, mtm);
        let delegates = self.delegates();
        palette.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn CommandPaletteViewDelegate>));

        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect(0.0, 0.0, palette.preferred_width(), 460.0),
            NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        panel.setTitlebarAppearsTransparent(true);
        if let Some(button) = panel.standardWindowButton(NSWindowButton::CloseButton) {
            button.setHidden(true);
        }
        if let Some(button) = panel.standardWindowButton(NSWindowButton::MiniaturizeButton) {
            button.setHidden(true);
        }
        if let Some(button) = panel.standardWindowButton(NSWindowButton::ZoomButton) {
            button.setHidden(true);
        }
        panel.setFloatingPanel(true);
        panel.setLevel(NSFloatingWindowLevel);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(false);
        // SAFETY: the panel is owned through `Retained` references; it must
        // not release itself on close.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setContentView(Some(&palette));

        let weak_self: ObjcWeak<DocumentWindowController> = ObjcWeak::from(self);
        let weak_panel: ObjcWeak<NSPanel> = ObjcWeak::from(&*panel);
        let close_handler = CommandPaletteWindowDelegate::new(
            move || {
                let Some(this) = weak_self.load() else { return };
                let panel = weak_panel.load();
                if let Some(panel) = &panel
                    && let Some(window) = this.window()
                {
                    window.removeChildWindow(panel);
                }
                let current = this.command_palette_window();
                let same = match (&current, &panel) {
                    (Some(current), Some(panel)) => Retained::as_ptr(current) == Retained::as_ptr(panel),
                    (None, None) => true,
                    _ => false,
                };
                if same {
                    this.set_command_palette_window(None);
                }
            },
            mtm,
        );
        panel.setDelegate(Some(ProtocolObject::from_ref(&*close_handler)));
        // SAFETY: the key is a static's address; the value is a live object
        // the association retains.
        unsafe {
            ffi::objc_setAssociatedObject(
                Retained::as_ptr(&panel) as *mut AnyObject,
                &COMMAND_PALETTE_ASSOCIATION_KEY as *const u8 as *const c_void,
                Retained::as_ptr(&close_handler) as *mut AnyObject,
                ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
        }

        self.set_command_palette_window(Some(panel.clone()));
        if let Some(parent) = self.window() {
            // SAFETY: both are live windows; the palette becomes a child.
            unsafe { parent.addChildWindow_ordered(&panel, NSWindowOrderingMode::Above) };
            let parent_frame = parent.frame();
            let origin = NSPoint::new(
                parent_frame.mid_x() - panel.frame().width() / 2.0,
                parent_frame.mid_y() - panel.frame().height() / 2.0 + 60.0,
            );
            panel.setFrameOrigin(origin);
        } else {
            panel.center();
        }
        panel.makeKeyAndOrderFront(None);
    }

    /// The providers Swift builds (twice) as the palette's argument list:
    /// the current document, the last 30 recent files, then
    /// `quickOpenProviders`.
    fn palette_providers(&self) -> Vec<Rc<dyn QuickOpenProvider>> {
        let mut providers: Vec<Rc<dyn QuickOpenProvider>> = vec![
            Rc::new(CurrentDocumentQuickOpenProvider::new(self.markdown_document().parsed())),
            Rc::new(RecentFilesQuickOpenProvider {
                files: DocumentStateStore::shared()
                    .recents(30)
                    .iter()
                    .map(|recent| FileUrl::from_path(&recent.path))
                    .collect(),
            }),
        ];
        providers.extend(self.quick_open_providers());
        providers
    }

    /// `commandPalette(_:didChoose:)`.
    pub fn command_palette_did_choose(&self, _palette: &CommandPaletteView, result: &QuickOpenResult) {
        if let Some(window) = self.command_palette_window() {
            window.close();
        }
        match &result.action {
            QuickOpenAction::Command(command) => {
                self.perform(*command);
            }
            QuickOpenAction::Select(range) => {
                let range = *range;
                if !(range.upper_bound() <= self.markdown_document().storage().length() as isize) {
                    return;
                }
                self.container_text_view().set_source_selected_ranges(&[range]);
                self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, true);
                if let Some(window) = self.window() {
                    make_first_responder(&window, &self.container_text_view());
                }
            }
            QuickOpenAction::Open(url) => {
                if let Some(delegate) = app_delegate(MainThreadMarker::from(self)) {
                    delegate.open(url, Some(self.mode()), None, false, DocumentOpenDisposition::Tab, None);
                }
            }
            QuickOpenAction::OpenAt(url, range) => {
                let range = *range;
                if !self.open_in_place(url) {
                    return;
                }
                if !(range.upper_bound() <= self.markdown_document().storage().length() as isize) {
                    return;
                }
                self.container_text_view().set_source_selected_ranges(&[range]);
                self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, false);
                if let Some(window) = self.window() {
                    make_first_responder(&window, &self.container_text_view());
                }
            }
        }
    }

    /// `commandPaletteDidCancel(_:)`.
    pub fn command_palette_did_cancel(&self, _palette: &CommandPaletteView) {
        if let Some(window) = self.command_palette_window() {
            window.close();
        }
    }
}

// MARK: - CommandPaletteViewDelegate

impl CommandPaletteViewDelegate for DocumentWindowControllerDelegates {
    fn command_palette_did_choose(&self, palette: &CommandPaletteView, result: &QuickOpenResult) {
        if let Some(controller) = self.controller() {
            controller.command_palette_did_choose(palette, result);
        }
    }

    fn command_palette_did_cancel(&self, palette: &CommandPaletteView) {
        if let Some(controller) = self.controller() {
            controller.command_palette_did_cancel(palette);
        }
    }
}

// MARK: - CommandPaletteWindowDelegate

pub struct CommandPaletteWindowDelegateIvars {
    on_close: Box<dyn Fn()>,
}

define_class!(
    /// `private final class CommandPaletteWindowDelegate: NSObject,
    /// NSWindowDelegate`.
    // SAFETY: `init` is forwarded to `NSObject` in `new` after the ivars are
    // set; `windowWillClose:` keeps AppKit's signature.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CommandPaletteWindowDelegate"]
    #[ivars = CommandPaletteWindowDelegateIvars]
    pub struct CommandPaletteWindowDelegate;

    unsafe impl NSObjectProtocol for CommandPaletteWindowDelegate {}

    unsafe impl NSWindowDelegate for CommandPaletteWindowDelegate {
        #[unsafe(method(windowWillClose:))]
        fn __window_will_close(&self, _notification: &NSNotification) {
            (self.ivars().on_close)();
        }
    }
);

impl CommandPaletteWindowDelegate {
    /// `init(onClose:)`.
    pub fn new(on_close: impl Fn() + 'static, mtm: MainThreadMarker) -> Retained<CommandPaletteWindowDelegate> {
        let this = Self::alloc(mtm).set_ivars(CommandPaletteWindowDelegateIvars { on_close: Box::new(on_close) });
        // SAFETY: `NSObject`'s designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}
