//! Port of `Panels/DocumentQuickLook.swift`: Quick Look, pointed inward.
//!
//! Three parts, as in Swift:
//!
//! * [`QuickLookRequest`]: what a Quick Look gesture on the document surface
//!   should actually open. A pure decision, testable headless.
//! * [`QuickLookSession`]: the one file the panel is showing, plus where it
//!   came from. Swift keeps it on the window controller through an
//!   associated object; so does the port (an `NSObject` subclass named
//!   `QuickLookSession`, stored with `objc_setAssociatedObject` under a
//!   private static key).
//! * The `extension DocumentWindowController: QLPreviewPanelDataSource,
//!   QLPreviewPanelDelegate`. `DocumentWindowController` belongs to the app
//!   shell (`App/`), so the extension is the [`QuickLookHost`] trait: the
//!   window controller implements its six required accessors, and the
//!   provided methods are the Swift bodies, unchanged. The controller's
//!   `define_class!` forwards these Objective-C methods to them:
//!
//!   | Objective-C (on `DocumentWindowController`)          | `QuickLookHost` method           |
//!   |------------------------------------------------------|----------------------------------|
//!   | `acceptsPreviewPanelControl:` → `BOOL`               | `accepts_preview_panel_control`  |
//!   | `beginPreviewPanelControl:`                          | `begin_preview_panel_control`    |
//!   | `endPreviewPanelControl:`                            | `end_preview_panel_control`      |
//!   | `numberOfPreviewItemsInPreviewPanel:` → `NSInteger`  | `number_of_preview_items`        |
//!   | `previewPanel:previewItemAtIndex:` → `id`            | `preview_item_at`                |
//!   | `previewPanel:sourceFrameOnScreenForPreviewItem:` → `NSRect` | `source_frame_on_screen` |
//!
//!   and its `MarkdownTextViewDelegate::wants_quick_look_for` returns
//!   `markdown_text_view_wants_quick_look_for`. `quick_look_at_selection`,
//!   `has_quick_look_target` and `present_quick_look` are the controller's
//!   other entry points.
//!
//! `MarkdownLinkDestination` lives in `DocumentWindowController+Delegates.swift`
//! (app shell, not ported on this branch); `resolve` needs its `classify`, so
//! this module carries a private copy of it with the same body. Replace it
//! with the app shell's type once that file is ported.
//!
//! `QLPreviewPanel` (Quartz) has no objc2 crate here; it is reached through
//! the runtime by name, with Quartz linked as Swift's `import Quartz` links
//! it.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_foundation::{NSRect, NSURL};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::RectExt;
use upleft_render::core_types::{NSRange, PathToken};
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::{ContextTarget, ContextTargetKind};
use upleft_swift_text as swift;

use crate::ai::path_resolver::Resolution;
use crate::assets::asset_resolver::url_with_string;

#[link(name = "Quartz", kind = "framework")]
unsafe extern "C" {}

// MARK: - QuickLookRequest

/// What a Quick Look gesture on the document surface should actually open.
///
/// A pure decision, separate from the panel, because the interesting part is
/// the routing and the routing has three different answers that all look
/// alike from the outside. The panel itself is untestable in a headless
/// suite; this is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickLookRequest {
    /// An embedded image opens in Downright's own lightbox, not in the system
    /// panel. The lightbox is already what a *click* on that image does
    /// (§7.1), it zooms and pans and shows the alt text as a caption, and
    /// having force click open a second, different image viewer for the same
    /// picture would be two answers to one question.
    Lightbox { source: String },
    /// Everything else — a path token, a link to a local file — is a
    /// question about a *file*, which is exactly what `QLPreviewPanel`
    /// answers, for every type the system has a generator for. Including
    /// Markdown: this app ships the Quick Look extension that renders it.
    Panel(FileUrl),
}

impl QuickLookRequest {
    /// Resolves a target, or reports that it is not previewable.
    ///
    /// Returning `None` matters as much as returning a request: a force click
    /// that resolves to nothing falls back to `NSTextView`'s Look Up popover,
    /// which is what must still happen on an ordinary word.
    pub fn resolve(
        target: &ContextTarget,
        document_url: Option<&FileUrl>,
        path_resolution: &dyn Fn(&PathToken) -> Option<Resolution>,
        file_exists: &dyn Fn(&FileUrl) -> bool,
    ) -> Option<QuickLookRequest> {
        match &target.kind {
            ContextTargetKind::Image(source) => Some(QuickLookRequest::Lightbox { source: source.clone() }),

            ContextTargetKind::PathToken(token) => {
                let resolution = path_resolution(token)?;
                if !resolution.exists {
                    return None;
                }
                let url = resolution.url?;
                Some(QuickLookRequest::Panel(url.standardized_file_url()))
            }

            ContextTargetKind::Link(destination) => match MarkdownLinkDestination::classify(destination) {
                MarkdownLinkDestination::LocalFile(url) => {
                    let target = standardized_file_url(&url)?;
                    if file_exists(&target) { Some(QuickLookRequest::Panel(target)) } else { None }
                }
                MarkdownLinkDestination::Relative(relative) => {
                    let base = document_url?.deleting_last_path_component();
                    let target = base.appending_path_component(&relative).standardized_file_url();
                    if file_exists(&target) { Some(QuickLookRequest::Panel(target)) } else { None }
                }
                // A web link, an in-document anchor, and an automation URL
                // are all things Quick Look cannot preview. Refused rather
                // than opened: turning a preview gesture into "launch this
                // URL" would route around the trust prompt that a real click
                // goes through.
                MarkdownLinkDestination::Web(_)
                | MarkdownLinkDestination::Anchor(_)
                | MarkdownLinkDestination::Automation(_)
                | MarkdownLinkDestination::Invalid => None,
            },

            ContextTargetKind::Heading(_)
            | ContextTargetKind::CodeBlock(_)
            | ContextTargetKind::Table(_)
            | ContextTargetKind::Selection
            | ContextTargetKind::Plain => None,
        }
    }
}

/// `URL(string:)`'s `standardizedFileURL` for a `file:` URL: a file URL of
/// the standardized path that keeps `hasDirectoryPath` (as
/// `asset_resolver` does for the same conversion). `None` where the URL has
/// no path, which Swift keeps as a URL that no file exists at.
fn standardized_file_url(url: &NSURL) -> Option<FileUrl> {
    let path = url.path().map(|path| path.to_string()).unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    Some(FileUrl::from_path_is_directory(&path, url.hasDirectoryPath()).standardized_file_url())
}

/// A private copy of `MarkdownLinkDestination`
/// (`App/DocumentWindowController+Delegates.swift`), which the app shell
/// owns; see the module notes.
enum MarkdownLinkDestination {
    Anchor(#[allow(dead_code)] String),
    Web(#[allow(dead_code)] Retained<NSURL>),
    LocalFile(Retained<NSURL>),
    Automation(#[allow(dead_code)] Retained<NSURL>),
    Relative(String),
    Invalid,
}

impl MarkdownLinkDestination {
    fn classify(destination: &str) -> MarkdownLinkDestination {
        if swift::has_prefix(destination, "#") {
            return MarkdownLinkDestination::Anchor(swift::drop_first(destination, 1).to_owned());
        }
        let url = url_with_string(destination);
        let scheme = url.as_ref().and_then(|url| url.scheme()).map(|scheme| swift::lowercased(&scheme.to_string()));
        let (Some(url), Some(scheme)) = (url, scheme) else {
            return if destination.is_empty() {
                MarkdownLinkDestination::Invalid
            } else {
                MarkdownLinkDestination::Relative(destination.to_owned())
            };
        };
        match scheme.as_str() {
            "http" | "https" | "mailto" => MarkdownLinkDestination::Web(url),
            "file" => MarkdownLinkDestination::LocalFile(url),
            _ => MarkdownLinkDestination::Automation(url),
        }
    }
}

// MARK: - QuickLookSession

pub struct QuickLookSessionIvars {
    url: RefCell<Option<FileUrl>>,
    /// Source range of the thing being previewed, so the panel can zoom out
    /// of the text it came from instead of appearing from the middle of
    /// nowhere.
    source_range: Cell<Option<NSRange>>,
}

define_class!(
    /// The one file the panel is currently showing, plus where it came from
    /// (`QuickLookSession`, private in Swift).
    ///
    /// Held on the controller through an associated object because
    /// `DocumentWindowController`'s stored properties live in a file this
    /// feature does not own; the sharing picker next door keeps its state
    /// the same way.
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSObject))]
    #[name = "QuickLookSession"]
    #[ivars = QuickLookSessionIvars]
    pub struct QuickLookSession;

    unsafe impl NSObjectProtocol for QuickLookSession {}
);

impl QuickLookSession {
    fn new() -> Retained<QuickLookSession> {
        let this = Self::alloc()
            .set_ivars(QuickLookSessionIvars { url: RefCell::new(None), source_range: Cell::new(None) });
        unsafe { msg_send![super(this), init] }
    }

    pub fn url(&self) -> Option<FileUrl> {
        self.ivars().url.borrow().clone()
    }

    pub fn set_url(&self, url: Option<FileUrl>) {
        *self.ivars().url.borrow_mut() = url;
    }

    pub fn source_range(&self) -> Option<NSRange> {
        self.ivars().source_range.get()
    }

    pub fn set_source_range(&self, range: Option<NSRange>) {
        self.ivars().source_range.set(range);
    }
}

/// `private var quickLookSessionKey: UInt8 = 0`: its address is the key.
static QUICK_LOOK_SESSION_KEY: u8 = 0;

/// `DocumentWindowController.quickLookSession`: the associated session,
/// created on first use.
pub fn quick_look_session(owner: &AnyObject) -> Retained<QuickLookSession> {
    let key = &QUICK_LOOK_SESSION_KEY as *const u8 as *const std::ffi::c_void;
    let existing = unsafe { objc2::ffi::objc_getAssociatedObject(owner as *const AnyObject as *const _, key) };
    if !existing.is_null() {
        let existing = unsafe { &*existing };
        if let Some(session) = super::appkit_support::downcast::<QuickLookSession>(existing) {
            return session;
        }
    }
    let session = QuickLookSession::new();
    unsafe {
        objc2::ffi::objc_setAssociatedObject(
            owner as *const AnyObject as *mut _,
            key,
            Retained::as_ptr(&session) as *mut AnyObject as *mut _,
            objc2::ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }
    session
}

// MARK: - DocumentWindowController extension

/// What the Swift extension reads from `DocumentWindowController`. The
/// provided methods are the extension's bodies.
pub trait QuickLookHost {
    /// The controller itself: the object the session hangs off, and the
    /// preview panel's data source and delegate.
    fn quick_look_owner(&self) -> Retained<AnyObject>;
    /// `containerTextView`.
    fn container_text_view(&self) -> Retained<MarkdownTextView>;
    /// `markdownDocument.url`.
    fn markdown_document_url(&self) -> Option<FileUrl>;
    /// `pathResolver?.resolve(token)` (the Swift closure captures the
    /// controller weakly; the host does the same).
    fn resolve_path_token(&self, token: &PathToken) -> Option<Resolution>;
    /// `presentLightbox(source:caption:)`.
    fn present_lightbox(&self, source: &str, caption: Option<&str>);
    /// `authorizeLocalEffect(.readLocalAsset, target: url) { action() }`.
    fn authorize_read_local_asset(&self, target: &FileUrl, action: Box<dyn FnOnce()>);

    // MARK: Entry points

    /// A force click landed on something (§7.1). Reported by the text view
    /// in source coordinates; the decision about what that means is here.
    fn markdown_text_view_wants_quick_look_for(&self, _view: &MarkdownTextView, target: &ContextTarget) -> bool
    {
        self.present_quick_look(target)
    }

    /// The Quick Look command, aimed at whatever the selection is on.
    fn quick_look_at_selection(&self) -> bool
    {
        let Some(target) = self.container_text_view().quick_look_target_at_selection() else { return false };
        self.present_quick_look(&target)
    }

    /// Whether the command should be offered at all. Reading the attribute
    /// runs at the caret is cheap; resolving the file is not, so the menu
    /// asks only the first question and the command asks the second.
    fn has_quick_look_target(&self) -> bool {
        self.container_text_view().quick_look_target_at_selection().is_some()
    }

    fn present_quick_look(&self, target: &ContextTarget) -> bool
    {
        let document_url = self.markdown_document_url();
        let request = QuickLookRequest::resolve(
            target,
            document_url.as_ref(),
            &|token| self.resolve_path_token(token),
            &|url| upleft_foundation::file_manager::file_exists(&url.path()),
        );
        let Some(request) = request else { return false };

        match request {
            QuickLookRequest::Lightbox { source } => self.present_lightbox(&source, None),
            QuickLookRequest::Panel(url) => {
                // Reading is the least the app can ask for, and it is the
                // same effect the lightbox and the Asset Doctor request.
                // Opening a preview is not launching the file, so this
                // deliberately does not ask for `.launchPathOrEditor`.
                let owner = self.quick_look_owner();
                let source_range = target.source_range;
                let panel_url = url.clone();
                let weak = objc2::rc::Weak::from_retained(&owner);
                self.authorize_read_local_asset(
                    &url,
                    Box::new(move || {
                        if let Some(owner) = weak.load() {
                            show_quick_look_panel(&owner, panel_url, source_range);
                        }
                    }),
                );
            }
        }
        // True either way: the gesture was aimed at a file and was handled,
        // including when the trust policy answered "ask" and the preview is
        // waiting on the reader. Falling through to the dictionary here
        // would pop a Look Up card over a filename.
        true
    }

    // MARK: Panel control (the responder-chain handshake)

    fn accepts_preview_panel_control(&self, _panel: &AnyObject) -> bool {
        true
    }

    fn begin_preview_panel_control(&self, panel: &AnyObject) {
        let owner = self.quick_look_owner();
        let _: () = unsafe { msg_send![panel, setDataSource: &*owner] };
        let _: () = unsafe { msg_send![panel, setDelegate: &*owner] };
    }

    fn end_preview_panel_control(&self, panel: &AnyObject) {
        // The panel outlives this window controller, so leaving it pointing
        // here would keep a closed document's controller alive and, worse,
        // answer for the next document opened in its place.
        let owner = self.quick_look_owner();
        let data_source: Option<Retained<AnyObject>> = unsafe { msg_send![panel, dataSource] };
        if data_source.is_some_and(|data_source| std::ptr::eq(Retained::as_ptr(&data_source), Retained::as_ptr(&owner))) {
            let _: () = unsafe { msg_send![panel, setDataSource: std::ptr::null::<AnyObject>()] };
        }
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![panel, delegate] };
        if delegate.is_some_and(|delegate| std::ptr::eq(Retained::as_ptr(&delegate), Retained::as_ptr(&owner))) {
            let _: () = unsafe { msg_send![panel, setDelegate: std::ptr::null::<AnyObject>()] };
        }
        quick_look_session(&owner).set_url(None);
        quick_look_session(&owner).set_source_range(None);
    }

    // MARK: Data source

    fn number_of_preview_items(&self, _panel: &AnyObject) -> isize {
        if quick_look_session(&self.quick_look_owner()).url().is_none() { 0 } else { 1 }
    }

    fn preview_item_at(&self, _panel: &AnyObject, _index: isize) -> Option<Retained<NSURL>> {
        quick_look_session(&self.quick_look_owner()).url().map(|url| url.to_nsurl())
    }

    // MARK: Delegate

    /// Zooms out of the text the preview came from. Without this the panel
    /// scales up from the centre of the screen, which reads as an unrelated
    /// window opening rather than as the path under the pointer expanding.
    fn source_frame_on_screen(&self, _panel: &AnyObject, _item: Option<&AnyObject>) -> NSRect {
        let Some(range) = quick_look_session(&self.quick_look_owner()).source_range() else { return NSRect::ZERO };
        let text_view = self.container_text_view();
        let Some(rect) = text_view.rect_for_offset(range.location) else { return NSRect::ZERO };
        let Some(window) = text_view.window() else { return NSRect::ZERO };
        let view: &NSView = &text_view;
        let in_window = view.convertRect_toView(rect, None);
        // Off-screen (the reader scrolled away, or the preview came from a
        // command rather than a click) means no anchor at all: `.zero` asks
        // the panel for its plain fade, which is better than an animation
        // flying out of a corner the target is not in.
        if !view.visibleRect().intersects(rect) {
            return NSRect::ZERO;
        }
        window.convertRectToScreen(in_window)
    }
}

/// `showQuickLookPanel(url:sourceRange:)` (private in Swift).
fn show_quick_look_panel(owner: &AnyObject, url: FileUrl, source_range: NSRange) {
    let session = quick_look_session(owner);
    session.set_url(Some(url));
    session.set_source_range(Some(source_range));
    let Some(class) = AnyClass::get(c"QLPreviewPanel") else { return };
    let panel: Option<Retained<AnyObject>> = unsafe { msg_send![class, sharedPreviewPanel] };
    let Some(panel) = panel else { return };
    let exists: bool = unsafe { msg_send![class, sharedPreviewPanelExists] };
    let visible: bool = unsafe { msg_send![&*panel, isVisible] };
    if exists && visible {
        // Already up on a different file — reload in place rather than
        // closing and reopening, which would flash the desktop through.
        let _: () = unsafe { msg_send![&*panel, reloadData] };
    } else {
        let _: () = unsafe { msg_send![&*panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()] };
    }
}
