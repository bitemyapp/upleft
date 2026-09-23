//! Port of `App/DocumentWindowController+Actions.swift`: the small actions the
//! context menus and delegates reach for, the toolbar (`NSToolbarDelegate`,
//! `NSMenuDelegate`, `NSToolbarItemValidation`), the Document/Source switch
//! and its snapshot transition, the `···` overflow menu, and command states.
//!
//! The Objective-C entry points of this extension (the toolbar delegate, the
//! menu delegate, toolbar validation, and the `toolbar…:` targets) are
//! declared in `document_window_controller.rs`'s `define_class!` and forward
//! to the methods below. `PresentationSnapshotView` (a private Swift class)
//! is a `define_class!` type of that Objective-C name.
//!
//! Main-thread I/O, as in Swift: `save_code_block` and
//! `open_code_block_in_editor` write their snippet on the main thread (the
//! first behind a modal save panel), and `save_image_copy` copies the image
//! there too. `present_lightbox` reads the image off the main thread, as
//! Swift's `Task.detached` does.

use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBeep, NSControl, NSControlStateValueOff, NSControlStateValueOn, NSImage,
    NSImageScaling, NSImageView, NSMenu, NSMenuItem, NSModalResponseOK, NSResponder, NSSavePanel, NSToolbar,
    NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem, NSToolbarItemVisibilityPriorityHigh,
    NSToolbarSpaceItemIdentifier, NSView, NSWindowOrderingMode,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{
    NSArray, NSData, NSDataWritingOptions, NSError, NSFileManager, NSNumber, NSPoint, NSRect, NSString, NSURL, NSValue,
};
use objc2_quartz_core::{
    CAAnimation, CAAnimationGroup, CABasicAnimation, CAMediaTiming, CATransaction, CATransform3D,
    NSValueCATransform3DAdditions,
};
use upleft_core::restructure::Restructure;
use upleft_core::swift_text::ns::{NSStringExt, utf16};
use upleft_core::{BlockContent, NSRange, TableAlignment, ZoomLevel};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::RectExt;
use upleft_render::fragments::local_asset_policy::LocalAssetPolicy;
use upleft_render::motion::{self, Curve};
use upleft_render::render_contracts::SourceFocus;
use upleft_render::swift_compat::{smax, smin};
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_swift_text as swift_text;

use crate::app::document_window_controller::DocumentWindowController;
use crate::app::main_menu::MainMenu;
use crate::app::presentation_drag::PresentationSwitchBudget;
use crate::app::toolbar_controls::{
    ToolbarActionButton, ToolbarDocumentIdentityView, ToolbarMenuButton, ToolbarPresentationControl,
    ToolbarTrailingCluster,
};
use crate::assets::asset_resolver::url_with_string;
use crate::panels::history_inspector_view::{HistoryInspectorView, HistoryInspectorViewDelegate};
use crate::panels::inspector_host_view::InspectorSection;
use crate::panels::lightbox_window::LightboxWindow;
use crate::panels::update_status_pill::{Presentation as UpdateStatusPillPresentation, UpdateStatusPill};
use crate::security::document_trust::TrustEffect;
use crate::support::commands::Command;
use crate::support::preferences::Preferences;

// MARK: - PresentationSnapshotView

define_class!(
    /// `private final class PresentationSnapshotView: NSImageView`: the still
    /// of the outgoing presentation, transparent to the pointer.
    // SAFETY: no ivars; `initWithFrame:` is NSImageView's own.
    #[unsafe(super(NSImageView, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PresentationSnapshotView"]
    pub struct PresentationSnapshotView;

    unsafe impl NSObjectProtocol for PresentationSnapshotView {}

    impl PresentationSnapshotView {
        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl PresentationSnapshotView {
    /// `PresentationSnapshotView(frame:)`.
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<PresentationSnapshotView> {
        unsafe { msg_send![PresentationSnapshotView::alloc(mtm), initWithFrame: frame] }
    }
}

// MARK: - Helpers

unsafe extern "C" {
    /// `<time.h>`: the clock in nanoseconds.
    fn clock_gettime_nsec_np(clock_id: u32) -> u64;
}

/// `CLOCK_UPTIME_RAW` (`<time.h>`): `mach_absolute_time` in nanoseconds.
const CLOCK_UPTIME_RAW: u32 = 8;

/// `DispatchTime.now().uptimeNanoseconds`: `DispatchTime.now()` reads
/// `mach_absolute_time()`, and `uptimeNanoseconds` converts it through the
/// timebase, which is `CLOCK_UPTIME_RAW`.
fn uptime_nanoseconds() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
}

/// `NSImage(systemSymbolName:accessibilityDescription: nil)`.
fn symbol_image(symbol: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None)
}

/// `NSMenuItem(title:action:keyEquivalent: "")`.
pub(crate) fn plain_menu_item(title: &str, action: Option<Sel>, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    // SAFETY: the action is answered with the Swift signature `(Any?)` by the
    // explicit target the callers set (or the responder chain).
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    }
}

/// A `CATransform3D` as the `NSValue` Swift's bridging produces.
fn transform_value(transform: CATransform3D) -> Retained<AnyObject> {
    // SAFETY: a plain value conversion.
    let value = unsafe { NSValue::valueWithCATransform3D(transform) };
    Retained::into_super(Retained::into_super(value))
}

fn identity_transform() -> CATransform3D {
    // SAFETY: an immutable framework constant.
    unsafe { objc2_quartz_core::CATransform3DIdentity }
}

/// `CATransform3DMakeTranslation(tx, ty, tz)`.
fn translation(tx: CGFloat, ty: CGFloat, tz: CGFloat) -> CATransform3D {
    CATransform3D::new_translation(tx, ty, tz)
}

/// An `Int` literal bridged to `Any` (`NSNumber(value: Int)`).
fn int_number(value: isize) -> Retained<AnyObject> {
    Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_isize(value))))
}

/// `Data(code.utf8).write(to: url)`: Foundation's non-atomic write, so the
/// error is Foundation's own.
fn write_utf8(code: &str, url: &NSURL) -> Result<(), Retained<NSError>> {
    let data = NSData::with_bytes(code.as_bytes());
    data.writeToURL_options_error(url, NSDataWritingOptions::empty())
}

/// `error.localizedDescription`.
fn localized_description(error: &NSError) -> String {
    upleft_swift_text::ns::foundation::to_string(&error.localizedDescription())
}

/// A value that crosses to the image queue and back. `NSURL` is immutable,
/// and the `NSImage` is created on the queue and only handed to the main
/// thread afterwards, as Swift's `Task.detached` hands it to `MainActor.run`.
struct CrossThread<T>(T);

// SAFETY: see the type's documentation; each value is used by one thread at
// a time, and the completion closure is only called on the main queue.
unsafe impl<T> Send for CrossThread<T> {}

/// `Task.detached(priority: .userInitiated) { NSImage(contentsOf: url) }`,
/// then `await MainActor.run { … }`.
fn load_image_off_main(url: Retained<NSURL>, completion: impl FnOnce(Option<Retained<NSImage>>) + 'static) {
    let url = CrossThread(url);
    let completion = CrossThread(Box::new(completion) as Box<dyn FnOnce(Option<Retained<NSImage>>)>);
    dispatch2::DispatchQueue::global_queue(dispatch2::GlobalQueueIdentifier::QualityOfService(
        dispatch2::DispatchQoS::UserInitiated,
    ))
    .exec_async(move || {
        let url = url;
        let image = objc2::rc::autoreleasepool(|_| NSImage::initWithContentsOfURL(NSImage::alloc(), &url.0));
        let payload = CrossThread((image, completion));
        dispatch2::DispatchQueue::main().exec_async(move || {
            let payload = payload;
            let (image, completion) = payload.0;
            (completion.0)(image);
        });
    });
}

// MARK: - Small actions

/// Small actions the context menus and delegates reach for.
impl DocumentWindowController {
    fn actions_mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    /// The controller as an Objective-C target.
    pub(crate) fn as_target(&self) -> &AnyObject {
        let object: &AnyObject = self;
        object
    }

    /// Follows a link to another markdown file in this same window (§7.1 —
    /// ⌘-click is what opens a new one). Persists the current document's
    /// state first so its reading position survives the hop.
    pub fn open_in_place(&self, url: &FileUrl) -> bool {
        if !self.confirm_pending_changes_before_close(false) {
            return false;
        }
        let previous_url = self.markdown_document().url();
        self.reset_transient_chrome();
        self.markdown_document().close();
        match self.open(url, self.mode()) {
            Ok(()) => {
                self.reset_workspace_state(url);
                true
            }
            Err(_) => {
                // The hop died mid-flight (the target vanished between the
                // click and the read). The document is closed and watcherless
                // now; put the file the reader was actually looking at back
                // instead of leaving a dead surface that no longer tracks
                // external writes.
                if let Some(previous_url) = previous_url {
                    if self.open(&previous_url, self.mode()).is_ok() {
                        self.reset_workspace_state(&previous_url);
                        return false;
                    }
                    // The previous file is gone too; fall through to the beep.
                }
                NSBeep();
                false
            }
        }
    }

    /// `updateBreadcrumbAndGutter()`.
    pub fn update_breadcrumb_and_gutter(&self) {
        self.refresh_breadcrumb();
        let current: Option<usize> =
            self.visible_heading_index(self.container_text_view().top_visible_offset()).map(|index| index as usize);
        let length = 1.max(self.markdown_document().parsed().length);
        let top = self.container_text_view().top_visible_offset() as CGFloat / length as CGFloat;
        let text_view = self.container_text_view();
        let active_container = self
            .document_panes()
            .into_iter()
            .find(|pane| std::ptr::eq(Retained::as_ptr(pane.text_view()), Retained::as_ptr(&text_view)))
            .unwrap_or_else(|| self.primary_container());
        let scroll_view = active_container.scroll_view();
        let visible_height = scroll_view.contentView().bounds().height();
        let document_height = smax(1.0, scroll_view.documentView().map_or(1.0, |view| view.bounds().height()));
        let span = smin(1.0, visible_height / document_height);
        let gutter = self.density_gutter_view();
        gutter.set_visible_range((top, smin(1.0, top + span)));
        gutter.set_read_progress(smax(gutter.read_progress(), smin(1.0, top + span)));
        // Only update outline entries / panel indices when the current heading
        // actually changes, to avoid creating a new array on every scroll frame.
        let entries = gutter.outline_entries();
        let previous_current = entries.iter().position(|entry| entry.is_current);
        if previous_current != current {
            gutter.set_outline_entries(
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, mut entry)| {
                        entry.is_current = Some(index) == current;
                        entry
                    })
                    .collect(),
            );
        }
    }

    // MARK: - Activity cue (§12)

    /// `beginActivity()`.
    pub fn begin_activity(&self) {
        self.activity_indicator().begin();
    }

    /// `endActivity()`.
    pub fn end_activity(&self) {
        self.activity_indicator().end();
    }

    // MARK: - Images

    /// `presentLightbox(source:caption:)`.
    pub fn present_lightbox(&self, source: &str, caption: Option<&str>) {
        if self.window().is_none() {
            return;
        }
        let document_url = self.markdown_document().url().map(|url| url.to_nsurl());
        let local_url = LocalAssetPolicy::request(source, document_url.as_deref()).map(|request| request.url);
        let remote_url = url_with_string(source).filter(|url| {
            let Some(scheme) = url.scheme().map(|scheme| swift_text::lowercased(&scheme.to_string())) else {
                return false;
            };
            (scheme == "http" || scheme == "https") && url.host().is_some()
        });
        let Some(url) = local_url.or(remote_url) else { return };
        let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
        let caption = caption.map(str::to_owned);
        let target = url.clone();
        let present = move || {
            let Some(this) = weak.load() else { return };
            for pane in this.document_panes() {
                pane.text_view().refresh_local_assets();
            }
            // Decoding — and for a remote image, downloading — must never
            // block the main thread: a slow or hanging server would freeze
            // the whole app inside the click handler. Read off-main, then
            // present on the main actor against the window that is current
            // when the image actually arrives.
            let style_sheet = this.active_style_sheet();
            let reduce_motion = style_sheet.reduce_motion;
            let reduce_transparency = style_sheet.reduce_transparency;
            let weak = ObjcWeak::new(&*this);
            let caption = caption.clone();
            load_image_off_main(target.clone(), move |image| {
                let Some(this) = weak.load() else { return };
                let Some(image) = image else { return };
                let Some(window) = this.window() else { return };
                LightboxWindow::new(&image, caption.as_deref(), reduce_motion, reduce_transparency, this.actions_mtm())
                    .present(&window);
            });
        };
        if url.isFileURL() {
            let Some(file_url) = FileUrl::from_nsurl(&url) else { return };
            self.authorize_local_effect(TrustEffect::ReadLocalAsset, &file_url, Rc::new(present));
        } else {
            self.authorize_remote_asset_url(&url, Rc::new(present));
        }
    }

    /// `saveImageCopy(source:)`.
    pub fn save_image_copy(&self, source: &str) {
        let document_url = self.markdown_document().url().map(|url| url.to_nsurl());
        let Some(origin) = LocalAssetPolicy::request(source, document_url.as_deref()).map(|request| request.url) else {
            return;
        };
        let Some(origin_file) = FileUrl::from_nsurl(&origin) else { return };
        let this = self.retain();
        self.authorize_local_effect(TrustEffect::ReadLocalAsset, &origin_file, Rc::new(move || {
            let panel = NSSavePanel::savePanel(this.actions_mtm());
            // `URL.lastPathComponent` is `""` where `NSURL` answers nil.
            panel.setNameFieldStringValue(&origin.lastPathComponent().unwrap_or_else(|| NSString::from_str("")));
            if panel.runModal() != NSModalResponseOK {
                return;
            }
            let Some(destination) = panel.URL() else { return };
            if let Err(error) = NSFileManager::defaultManager().copyItemAtURL_toURL_error(&origin, &destination) {
                this.present_operation_error("Couldn’t save the image copy", &localized_description(&error));
            }
        }));
    }

    // MARK: - Code blocks

    fn code_block_contents(&self, range: NSRange) -> String {
        let source = utf16(&self.markdown_document().text());
        if source.length() <= 0 {
            return String::new();
        }
        if let Some(block) = self.markdown_document().parsed().root.block_at(range.location)
            && let BlockContent::CodeBlock { content_range, .. } = block.content
            && content_range.location >= 0
            && content_range.upper_bound() <= source.length()
        {
            return source.substring(content_range);
        }

        if !(range.location >= 0 && range.upper_bound() <= source.length()) {
            return String::new();
        }
        let substring = source.substring(range);
        let mut lines: Vec<&str> = substring.split('\n').collect();
        if lines.first().is_some_and(|line| swift_text::has_prefix(swift_text::trim_whitespaces(line), "```")) {
            lines.remove(0);
        }
        if lines.last().is_some_and(|line| swift_text::has_prefix(swift_text::trim_whitespaces(line), "```")) {
            lines.pop();
        }
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// `saveCodeBlock(range:)`.
    pub fn save_code_block(&self, range: NSRange) {
        let code = self.code_block_contents(range);
        let panel = NSSavePanel::savePanel(self.actions_mtm());
        panel.setNameFieldStringValue(&NSString::from_str("snippet.txt"));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(url) = panel.URL() else { return };
        if let Err(error) = write_utf8(&code, &url) {
            self.present_operation_error("Couldn’t save the code block", &localized_description(&error));
        }
    }

    /// Fenced code has no file of its own, so "open in editor" writes it to a
    /// temp file first — the point is to get it into the user's editor, not to
    /// pretend it came from somewhere.
    pub fn open_code_block_in_editor(&self, range: NSRange) {
        let code = self.code_block_contents(range);
        let file_manager = NSFileManager::defaultManager();
        let Some(directory) = file_manager
            .temporaryDirectory()
            .URLByAppendingPathComponent_isDirectory(&NSString::from_str("Upleft"), true)
        else {
            return;
        };

        let mut name = "snippet.txt".to_owned();
        if let Some(block) = self.markdown_document().parsed().root.block_at(range.location)
            && let BlockContent::CodeBlock { language: Some(language), .. } = &block.content
        {
            name = format!("snippet.{}", CodeFileExtensions::extension(language));
        }
        let Some(url) = directory.URLByAppendingPathComponent(&NSString::from_str(&name)) else { return };
        // SAFETY: no attributes are passed.
        let written = unsafe {
            file_manager.createDirectoryAtURL_withIntermediateDirectories_attributes_error(&directory, true, None)
        }
        .and_then(|()| write_utf8(&code, &url));
        if let Err(error) = written {
            self.present_operation_error("Couldn’t prepare the code block", &localized_description(&error));
            return;
        }
        let Some(file_url) = FileUrl::from_nsurl(&url) else { return };
        let target = file_url.clone();
        self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &file_url, Rc::new(move || {
            Preferences::shared().values().external_editor.open(&target, None);
        }));
    }

    // MARK: - Tables (§6.3)

    /// `tableInsertRow(_:at:)`.
    pub fn table_insert_row(&self, table_range: NSRange, hit_offset: Option<isize>) {
        self.markdown_document().ensure_parsed_current();
        let row = self.row_index(table_range, hit_offset);
        let edits = Restructure::insert_row(&self.markdown_document().parsed(), table_range, row);
        self.markdown_document().apply(&edits, "Insert Row", None);
    }

    /// `tableDeleteRow(_:at:)`.
    pub fn table_delete_row(&self, table_range: NSRange, hit_offset: Option<isize>) {
        self.markdown_document().ensure_parsed_current();
        let row = self.row_index(table_range, hit_offset);
        let edits = Restructure::delete_row(&self.markdown_document().parsed(), table_range, row);
        self.markdown_document().apply(&edits, "Delete Row", None);
    }

    /// `tableSetAlignment(_:_:at:)`.
    pub fn table_set_alignment(&self, table_range: NSRange, alignment: TableAlignment, hit_offset: Option<isize>) {
        self.markdown_document().ensure_parsed_current();
        let column = self.column_index(table_range, hit_offset);
        let edits =
            Restructure::set_column_alignment(&self.markdown_document().parsed(), table_range, column, alignment);
        self.markdown_document().apply(&edits, "Set Column Alignment", None);
    }

    fn row_index(&self, table_range: NSRange, hit_offset: Option<isize>) -> isize {
        let Some(block) = self.markdown_document().parsed().root.block_at(table_range.location) else { return 0 };
        let BlockContent::Table(data) = &block.content else { return 0 };
        let offset = hit_offset.unwrap_or_else(|| self.caret_offset());
        data.rows.iter().position(|row| row.range.touches(offset)).map_or(0, |index| index as isize)
    }

    fn column_index(&self, table_range: NSRange, hit_offset: Option<isize>) -> isize {
        let Some(block) = self.markdown_document().parsed().root.block_at(table_range.location) else { return 0 };
        let BlockContent::Table(data) = &block.content else { return 0 };
        let offset = hit_offset.unwrap_or_else(|| self.caret_offset());
        for row in &data.rows {
            if let Some(index) = row.cells.iter().position(|cell| cell.range.touches(offset)) {
                return index as isize;
            }
        }
        0
    }
}

/// File extension for a fence language, used when handing a snippet to an
/// external editor so its own syntax highlighting kicks in.
pub struct CodeFileExtensions;

impl CodeFileExtensions {
    /// `extension(for:)`.
    pub fn extension(language: &str) -> &'static str {
        const TABLE: &[(&[&str], &str)] = &[
            (&["swift"], "swift"),
            (&["typescript", "ts"], "ts"),
            (&["tsx"], "tsx"),
            (&["javascript", "js"], "js"),
            (&["jsx"], "jsx"),
            (&["python", "py"], "py"),
            (&["rust", "rs"], "rs"),
            (&["go"], "go"),
            (&["ruby", "rb"], "rb"),
            (&["java"], "java"),
            (&["c"], "c"),
            (&["cpp", "c++"], "cpp"),
            (&["objc", "objective-c"], "m"),
            (&["bash", "sh", "shell", "zsh"], "sh"),
            (&["json"], "json"),
            (&["yaml", "yml"], "yaml"),
            (&["toml"], "toml"),
            (&["sql"], "sql"),
            (&["html"], "html"),
            (&["css"], "css"),
            (&["xml"], "xml"),
            (&["markdown", "md"], "md"),
        ];
        let lowered = swift_text::lowercased(language);
        // Swift's `switch` on a `String` compares with `==`: canonical
        // equivalence. Every pattern is ASCII, which is its own NFC.
        for (patterns, extension) in TABLE {
            if patterns.iter().any(|pattern| swift_text::str_eq(&lowered, pattern)) {
                return extension;
            }
        }
        "txt"
    }
}

// MARK: - Toolbar
//
// The toolbar has three stable zones: document identity at the leading edge,
// Document/Source at the optical centre, and a compact trailing cluster —
// activity, the panels a reader reaches for, task progress, overflow — that
// never nudges the centre rail.
//
// Find is in that cluster rather than inside `···` on purpose. The overflow
// was the only interactive control on the trailing edge, which put a frequent
// document action one unlabelled glyph and one menu away in a window with
// room for another button. `···` keeps what a reader reaches for
// occasionally; Find stays visible because it is used constantly.
//
// The cluster ships as a single toolbar item, not five: AppKit pads every
// custom-view item by its own margin — a tax even a hidden 1pt placeholder
// pays — which scattered the row with uneven 14pt/42pt gaps and stretched the
// button plates to 36pt beside the ring's 30pt one. `ToolbarTrailingCluster`
// owns the spacing instead, so the row reads as one tight unit against the
// trailing edge and the hidden spinner and pill cost nothing.

impl DocumentWindowController {
    /// `private static let identityItem`.
    pub const IDENTITY_ITEM: &'static str = "document-identity";
    /// `static let modeItem = NSToolbarItem.Identifier("presentation-mode")`.
    pub const MODE_ITEM: &'static str = "presentation-mode";
    /// `private static let clusterItem`.
    pub const CLUSTER_ITEM: &'static str = "trailing-cluster";

    /// `toolbarDefaultItemIdentifiers(_:)`.
    pub fn toolbar_default_item_identifiers(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
        // SAFETY: AppKit's immutable identifier constant.
        let flexible_space = unsafe { NSToolbarFlexibleSpaceItemIdentifier };
        NSArray::from_retained_slice(&[
            NSString::from_str(Self::IDENTITY_ITEM),
            flexible_space.retain(),
            NSString::from_str(Self::MODE_ITEM),
            flexible_space.retain(),
            NSString::from_str(Self::CLUSTER_ITEM),
        ])
    }

    /// `toolbarAllowedItemIdentifiers(_:)`.
    pub fn toolbar_allowed_item_identifiers(&self, toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
        let mut identifiers = self.toolbar_default_item_identifiers(toolbar).to_vec();
        // SAFETY: AppKit's immutable identifier constants.
        unsafe {
            identifiers.push(NSToolbarSpaceItemIdentifier.retain());
            identifiers.push(NSToolbarFlexibleSpaceItemIdentifier.retain());
        }
        NSArray::from_retained_slice(&identifiers)
    }

    /// `toolbar(_:itemForItemIdentifier:willBeInsertedIntoToolbar:)`.
    pub fn toolbar_item_for_item_identifier(
        &self,
        _toolbar: &NSToolbar,
        identifier: &NSString,
        _will_be_inserted: bool,
    ) -> Option<Retained<NSToolbarItem>> {
        let mtm = self.actions_mtm();
        let name = identifier.to_string();
        if name == Self::IDENTITY_ITEM {
            let window = self.window()?;
            let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), identifier);
            let identity = ToolbarDocumentIdentityView::new(&window, mtm);
            identity.set_document_state(self.markdown_document().presentation_state());
            item.setView(Some(&identity));
            self.set_toolbar_document_identity_view(Some(&identity));
            item.setBordered(false);
            item.setLabel(&NSString::from_str("Document"));
            item.setVisibilityPriority(NSToolbarItemVisibilityPriorityHigh);
            Some(item)
        } else if name == Self::MODE_ITEM {
            let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
            let control = ToolbarPresentationControl::new(
                move |selected_segment| {
                    if let Some(this) = weak.load() {
                        this.change_presentation(selected_segment);
                    }
                },
                mtm,
            );
            control.setHidden(false);
            self.set_toolbar_presentation_control(Some(&control));
            let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), identifier);
            item.setView(Some(&control));
            item.setBordered(false);
            item.setLabel(&NSString::from_str("Document / Source"));
            item.setToolTip(Some(&NSString::from_str("Switch between rendered Document and Source Focus")));
            item.setVisibilityPriority(NSToolbarItemVisibilityPriorityHigh);
            self.wire_toolbar_action(&item, "Toggle Document / Source", sel!(toolbarToggleSourceFocus:));
            Some(item)
        } else if name == Self::CLUSTER_ITEM {
            let find_button = ToolbarActionButton::new(
                "magnifyingglass",
                "Find",
                "Find in this document",
                Some(self.as_target()),
                sel!(toolbarShowFind:),
                true,
                mtm,
            );
            find_button.set_style_sheet(self.active_style_sheet());
            self.set_toolbar_find_button(Some(&find_button));
            let pill = self
                .update_status_pill()
                .unwrap_or_else(|| UpdateStatusPill::new(UpdateStatusPillPresentation::Standard, mtm));
            self.set_update_status_pill(Some(pill.clone()));
            // Held weakly because History and Context fly out of it: a morph
            // needs the seat of the control the reader actually clicked, and
            // for everything behind `···` that control is this button.
            let overflow_button = ToolbarMenuButton::new(&self.make_overflow_menu(), mtm);
            self.set_toolbar_overflow_button(Some(&overflow_button));
            let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), identifier);
            let progress_ring = self.progress_ring().clone();
            let activity_indicator = self.activity_indicator().clone();
            let views: [&NSView; 5] = [&activity_indicator, &find_button, &progress_ring, &pill, &overflow_button];
            item.setView(Some(&ToolbarTrailingCluster::new(&views, mtm)));
            item.setBordered(false);
            item.setLabel(&NSString::from_str("Actions"));
            // The ring is a permanent control — it shows an empty track
            // rather than hiding on a document with no tasks — so the cluster
            // must not be the first item the toolbar drops, and each view's
            // own tooltip is richer than a static one here.
            item.setVisibilityPriority(NSToolbarItemVisibilityPriorityHigh);
            let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
            progress_ring.set_on_activate(Some(Rc::new(move || {
                if let Some(this) = weak.load() {
                    this.toolbar_show_tasks(None);
                }
            })));
            let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
            progress_ring.set_on_visibility_change(Some(Rc::new(move |_| {
                if let Some(toolbar) = weak.load().and_then(|this| this.window()).and_then(|window| window.toolbar()) {
                    toolbar.validateVisibleItems();
                }
            })));
            let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
            activity_indicator.set_on_visibility_change(Some(Rc::new(move |_| {
                if let Some(toolbar) = weak.load().and_then(|this| this.window()).and_then(|window| window.toolbar()) {
                    toolbar.validateVisibleItems();
                }
            })));
            // When the window narrows enough to drop the cluster, the
            // toolbar's overflow chevron shows this in its place; the submenu
            // keeps the panels one reach away.
            let menu_rep = plain_menu_item("Actions", None, mtm);
            let rep_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Actions"));
            rep_menu.addItem(&self.symbol_menu_item("Find", "magnifyingglass", sel!(toolbarShowFind:)));
            rep_menu.addItem(&self.symbol_menu_item("Tasks", "checkmark.circle", sel!(toolbarShowTasks:)));
            rep_menu.addItem(&self.symbol_menu_item(
                "Check for Updates…",
                "arrow.triangle.2.circlepath",
                sel!(toolbarCheckForUpdates:),
            ));
            menu_rep.setSubmenu(Some(&rep_menu));
            item.setMenuFormRepresentation(Some(&menu_rep));
            Some(item)
        } else {
            None
        }
    }

    fn wire_toolbar_action(&self, item: &NSToolbarItem, title: &str, action: Sel) {
        // SAFETY: the controller answers `action` with the Swift signature
        // `(Any?)`, and owns the window that owns the toolbar.
        unsafe {
            item.setTarget(Some(self.as_target()));
            item.setAction(Some(action));
        }
        let menu_item = plain_menu_item(title, Some(action), self.actions_mtm());
        // SAFETY: as above.
        unsafe { menu_item.setTarget(Some(self.as_target())) };
        item.setMenuFormRepresentation(Some(&menu_item));
    }

    /// `validateToolbarItem(_:)`.
    pub fn validate_toolbar_item(&self, _item: &NSToolbarItem) -> bool {
        // Every toolbar control is always reachable: the mode switch, and the
        // cluster's panels, ring, pill, and menu. The pill decides for itself
        // what a click opens, based on the update coordinator's state.
        true
    }

    /// `presentationSegment`: the presentation the document is actually in,
    /// `0` Document, `1` Source. Read from the text surface rather than the
    /// rail, which a swipe in flight has scrubbed somewhere between the two.
    pub fn presentation_segment(&self) -> isize {
        if self.primary_container().text_view().source_focus() != SourceFocus::None
            || self.split_container().is_some_and(|split| split.text_view().source_focus() != SourceFocus::None)
        {
            1
        } else {
            0
        }
    }

    /// `documentLineCount`: how many lines the open document has. The swipe
    /// asks, because whether it can afford to drag the next presentation in
    /// under the fingers is a question about this document's length.
    pub fn document_line_count(&self) -> isize {
        self.markdown_document().parsed().line_starts.len() as isize
    }

    /// Switch presentation with no transition of its own.
    ///
    /// The live drag takes this rather than `change_presentation`: it is
    /// already drawing the transition itself, it drives the rail itself, and
    /// it performs the switch *behind* a still of the outgoing page — so a
    /// second animation would flicker and a rail refresh would snap the
    /// indicator out from under the fingers. It also has to be able to put
    /// the mode back when a drag is abandoned, which an animated switch
    /// cannot do quietly.
    pub fn set_presentation_segment(&self, segment: isize) {
        let show_source = segment == 1;
        let started = uptime_nanoseconds();
        for pane in self.document_panes() {
            if show_source {
                pane.text_view().focus_entire_source();
            } else {
                pane.text_view().clear_source_focus();
            }
        }
        self.record_presentation_switch_cost(started);
    }

    /// Feed what the switch actually cost back into the swipe's budget, so
    /// the estimate is calibrated on this machine and this document rather
    /// than on the one the constant was measured on.
    fn record_presentation_switch_cost(&self, started: u64) {
        let elapsed = (uptime_nanoseconds() - started) as f64 / 1_000_000.0;
        PresentationSwitchBudget::record(elapsed, self.document_line_count());
    }

    /// `changePresentation(to:)`: switch presentation. Shared by the rail and
    /// the two-finger swipe, so both land on exactly one transition and one
    /// cost — the swipe adds no rendering of its own precisely because this
    /// is not cheap.
    pub fn change_presentation(&self, selected_segment: isize) {
        let show_source = selected_segment == 1;
        let started = uptime_nanoseconds();
        for pane in self.document_panes() {
            let outgoing = if self.active_style_sheet().reduce_motion { None } else { self.snapshot(&pane) };
            if show_source {
                pane.text_view().focus_entire_source();
            } else {
                pane.text_view().clear_source_focus();
            }
            self.animate_presentation_change(&pane, outgoing, show_source);
        }
        self.record_presentation_switch_cost(started);
        self.refresh_source_focus_toolbar();
        // The mode control should change presentation, then return the user
        // to the editor. Leaving first responder on the toolbar makes an
        // editable Document mode feel inert until a second click.
        if let Some(window) = self.window() {
            let text_view = self.primary_container().text_view().clone();
            window.makeFirstResponder(Some(&text_view));
        }
    }

    fn snapshot(&self, view: &NSView) -> Option<Retained<NSImageView>> {
        let bounds = view.bounds();
        if bounds.is_empty() {
            return None;
        }
        let representation = view.bitmapImageRepForCachingDisplayInRect(bounds)?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &representation);
        let image = NSImage::initWithSize(NSImage::alloc(), bounds.size);
        image.addRepresentation(&representation);
        let snapshot = PresentationSnapshotView::new(bounds, self.actions_mtm());
        snapshot.setImage(Some(&image));
        snapshot.setImageScaling(NSImageScaling::ScaleAxesIndependently);
        snapshot.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        snapshot.setWantsLayer(true);
        if let Some(layer) = snapshot.layer() {
            layer.setMasksToBounds(true);
        }
        view.addSubview_positioned_relativeTo(&snapshot, NSWindowOrderingMode::Above, None);
        Some(Retained::into_super(snapshot))
    }

    /// Keep the viewport fixed while the two presentations pass each other.
    /// The direction communicates the state change; the short overlap hides
    /// TextKit's synchronous fragment rebuild without turning it into a
    /// flash.
    fn animate_presentation_change(
        &self,
        pane: &MarkdownContainerView,
        outgoing: Option<Retained<NSImageView>>,
        show_source: bool,
    ) {
        let text_view = pane.text_view();
        let layers = if self.active_style_sheet().reduce_motion {
            None
        } else {
            outgoing.as_ref().and_then(|outgoing| Some((text_view.layer()?, outgoing.layer()?)))
        };
        let (Some(outgoing), Some((incoming_layer, outgoing_layer))) = (outgoing.clone(), layers) else {
            if let Some(outgoing) = outgoing {
                outgoing.removeFromSuperview();
            }
            return;
        };
        let direction: CGFloat = if show_source { 1.0 } else { -1.0 };
        text_view.setWantsLayer(true);
        let transform_key = NSString::from_str("transform");
        let opacity_key = NSString::from_str("opacity");
        let incoming_transform = CABasicAnimation::animationWithKeyPath(Some(&transform_key));
        // SAFETY: an `NSValue` of a `CATransform3D` is what `transform`
        // animates, an `NSNumber` what `opacity` animates.
        unsafe {
            incoming_transform.setFromValue(Some(&transform_value(translation(12.0 * direction, 0.0, 0.0))));
            incoming_transform.setToValue(Some(&transform_value(identity_transform())));
        }
        let incoming_opacity = CABasicAnimation::animationWithKeyPath(Some(&opacity_key));
        unsafe {
            incoming_opacity.setFromValue(Some(&int_number(0)));
            incoming_opacity.setToValue(Some(&int_number(1)));
        }
        let incoming = CAAnimationGroup::animation();
        incoming.setAnimations(Some(&animations(&[&incoming_transform, &incoming_opacity])));
        incoming.setDuration(motion::LIQUID_SETTLE);
        incoming.setTimingFunction(Some(&motion::timing(Curve::Decelerate)));
        incoming_layer.setOpacity(1.0);
        incoming_layer.setTransform(identity_transform());
        incoming_layer.addAnimation_forKey(&incoming, Some(&NSString::from_str("presentation-in")));

        let outgoing_transform = CABasicAnimation::animationWithKeyPath(Some(&transform_key));
        unsafe {
            outgoing_transform.setFromValue(Some(&transform_value(identity_transform())));
            outgoing_transform.setToValue(Some(&transform_value(translation(-9.0 * direction, 0.0, 0.0))));
        }
        let outgoing_opacity = CABasicAnimation::animationWithKeyPath(Some(&opacity_key));
        unsafe {
            outgoing_opacity.setFromValue(Some(&int_number(1)));
            outgoing_opacity.setToValue(Some(&int_number(0)));
        }
        let leaving = CAAnimationGroup::animation();
        leaving.setAnimations(Some(&animations(&[&outgoing_transform, &outgoing_opacity])));
        leaving.setDuration(motion::DELIBERATE);
        leaving.setTimingFunction(Some(&motion::timing(Curve::Structural)));
        CATransaction::begin();
        let weak_outgoing: ObjcWeak<NSImageView> = ObjcWeak::new(&outgoing);
        let completion = RcBlock::new(move || {
            if let Some(outgoing) = weak_outgoing.load() {
                outgoing.removeFromSuperview();
            }
        });
        // SAFETY: the block runs on the main thread, where the transaction
        // commits.
        unsafe { CATransaction::setCompletionBlock(Some(&completion)) };
        outgoing_layer.setOpacity(0.0);
        outgoing_layer.addAnimation_forKey(&leaving, Some(&NSString::from_str("presentation-out")));
        CATransaction::commit();
    }

    /// `refreshSourceFocusToolbar()`.
    pub fn refresh_source_focus_toolbar(&self) {
        let is_active = self.primary_container().text_view().source_focus() != SourceFocus::None
            || self.split_container().is_some_and(|split| split.text_view().source_focus() != SourceFocus::None);
        if let Some(control) = self.toolbar_presentation_control() {
            control.setHidden(false);
        }
        if let Some(control) = self.toolbar_presentation_control() {
            control.set_selected_segment(if is_active { 1 } else { 0 });
        }
    }

    /// `@objc toolbarShowTasks(_:)`.
    pub fn toolbar_show_tasks(&self, _sender: Option<&AnyObject>) {
        self.toggle_task_panel();
    }

    /// `@objc toolbarToggleSourceFocus(_:)`.
    pub fn toolbar_toggle_source_focus(&self, _sender: Option<&AnyObject>) {
        let segment = if self.primary_container().text_view().source_focus() == SourceFocus::None { 1 } else { 0 };
        self.change_presentation(segment);
    }

    /// `@objc toolbarShowHistory(_:)`.
    pub fn toolbar_show_history(&self, _sender: Option<&AnyObject>) {
        self.show_history_inspector();
    }

    /// `@objc toolbarShowFind(_:)`.
    pub fn toolbar_show_find(&self, _sender: Option<&AnyObject>) {
        if self.find_bar().is_some() && self.search_inspector().is_none() {
            self.dismiss_find_bar();
        } else {
            self.perform(Command::Find);
        }
    }

    /// `@objc toolbarCheckForUpdates(_:)`.
    pub fn toolbar_check_for_updates(&self, _sender: Option<&AnyObject>) {
        self.perform(Command::CheckForUpdates);
    }

    fn make_overflow_menu(&self) -> Retained<NSMenu> {
        let mtm = self.actions_mtm();
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("More"));
        menu.setDelegate(Some(ProtocolObject::from_ref(self)));

        menu.addItem(&section_header("Panels", mtm));
        menu.addItem(&self.symbol_menu_item("Tasks", "checkmark.circle", sel!(toolbarShowTasks:)));
        menu.addItem(&self.symbol_menu_item("History", "clock.arrow.circlepath", sel!(toolbarShowHistory:)));

        menu.addItem(&section_header("View", mtm));
        self.add_commands(&[Command::SourceMode, Command::FocusMode, Command::SplitView, Command::PinWindow], &menu);

        menu.addItem(&section_header("Document", mtm));
        let zoom = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Document Detail"));
        self.add_commands(
            &[Command::ZoomLevel1, Command::ZoomLevel2, Command::ZoomLevel3, Command::ZoomLevel4, Command::ZoomLevel5],
            &zoom,
        );
        zoom.addItem(&NSMenuItem::separatorItem(mtm));
        self.add_commands(&[Command::ZoomIn, Command::ZoomOut], &zoom);
        let zoom_item = plain_menu_item("Document Detail", None, mtm);
        zoom_item.setImage(symbol_image("text.magnifyingglass").as_deref());
        zoom_item.setSubmenu(Some(&zoom));
        menu.addItem(&zoom_item);
        self.add_commands(&[Command::TidyDocument, Command::ReaderProfiles], &menu);

        menu.addItem(&section_header("Share", mtm));
        // The section header has always said Share; until now everything
        // under it wrote a file the reader then had to go and find.
        self.add_commands(&[Command::Share, Command::ShareAsPdf], &menu);
        let export = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Export"));
        self.add_commands(&[Command::ExportPdf, Command::ExportHtml, Command::ExportSelectionAsImage], &export);
        let export_item = plain_menu_item("Export", None, mtm);
        export_item.setImage(symbol_image("square.and.arrow.up").as_deref());
        export_item.setSubmenu(Some(&export));
        menu.addItem(&export_item);
        menu
    }

    fn add_commands(&self, commands: &[Command], menu: &NSMenu) {
        for &command in commands {
            let item = MainMenu::command_item(command, self.actions_mtm());
            // SAFETY: the controller answers `performDownrightCommand:`.
            unsafe { item.setTarget(Some(self.as_target())) };
            menu.addItem(&item);
        }
    }

    /// `menuItem(title:symbol:action:)` (private in Swift).
    fn symbol_menu_item(&self, title: &str, symbol: &str, action: Sel) -> Retained<NSMenuItem> {
        let item = plain_menu_item(title, Some(action), self.actions_mtm());
        // SAFETY: the controller answers `action` with the Swift signature.
        unsafe { item.setTarget(Some(self.as_target())) };
        item.setImage(symbol_image(symbol).as_deref());
        item
    }

    /// `showHistoryInspector()`.
    pub fn show_history_inspector(&self) {
        let existing = self.history_inspector();
        let inspector = existing
            .clone()
            .unwrap_or_else(|| HistoryInspectorView::new(self.active_style_sheet(), self.actions_mtm()));
        if existing.is_none() {
            let delegate: std::rc::Weak<dyn HistoryInspectorViewDelegate> = Rc::downgrade(&self.delegates()) as _;
            inspector.set_delegate(Some(delegate));
            self.set_history_inspector(Some(inspector.clone()));
        }
        inspector.set_versions(self.markdown_document().versions());
        self.show_in_inspector(&inspector, InspectorSection::History);
    }

    /// `refreshToolbarSelectionState()`.
    pub fn refresh_toolbar_selection_state(&self) {
        self.refresh_source_focus_toolbar();
        self.refresh_toolbar_panel_buttons();
    }

    /// Keeps the promoted panel buttons lit in step with the panels they
    /// open, the same way the task ring lights when its own panel is showing
    /// — a control that opens a panel and then says nothing about it is how
    /// a toolbar stops being trustworthy.
    pub fn refresh_toolbar_panel_buttons(&self) {
        if let Some(button) = self.toolbar_find_button() {
            button.set_style_sheet(self.active_style_sheet());
        }
        if let Some(button) = self.toolbar_find_button() {
            button.set_is_on(self.find_bar().is_some());
        }
    }

    /// `menuNeedsUpdate(_:)`.
    pub fn menu_needs_update(&self, menu: &NSMenu) {
        MainMenu::refresh_key_equivalents(menu);
        for item in menu.itemArray().iter() {
            match item.title().to_string().as_str() {
                "Tasks" => item.setState(if self.is_task_panel_floating() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                }),
                "History" => item.setState(
                    if self.floating_surface().is_some()
                        && self.inspector_host().and_then(|host| host.selected_section())
                            == Some(InspectorSection::History)
                    {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    },
                ),
                _ => {}
            }
        }
        self.update_command_states(menu);
    }

    fn update_command_states(&self, menu: &NSMenu) {
        for item in menu.itemArray().iter() {
            if let Some(command) = MainMenu::command(&item) {
                item.setState(if self.command_state(command) { NSControlStateValueOn } else { NSControlStateValueOff });
            }
            if let Some(submenu) = item.submenu() {
                self.update_command_states(&submenu);
            }
        }
    }

    /// `commandState(_:)`.
    pub fn command_state(&self, command: Command) -> bool {
        match command {
            Command::FocusMode => self.is_focus_mode_enabled(),
            Command::SplitView => self.split_view_container().is_some(),
            Command::PinWindow => self.is_window_pinned(),
            Command::TypewriterScrolling => Preferences::shared().values().typewriter_scrolling,
            Command::StatusBar => Preferences::shared().values().show_status_bar,
            Command::SourceMode => {
                self.primary_container().text_view().source_focus() != SourceFocus::None
                    || self.split_container().is_some_and(|split| split.text_view().source_focus() != SourceFocus::None)
            }
            Command::ZoomLevel1 => self.container_text_view().zoom_level() == ZoomLevel::H1,
            Command::ZoomLevel2 => self.container_text_view().zoom_level() == ZoomLevel::H2,
            Command::ZoomLevel3 => self.container_text_view().zoom_level() == ZoomLevel::Headings,
            Command::ZoomLevel4 => self.container_text_view().zoom_level() == ZoomLevel::Skeleton,
            Command::ZoomLevel5 => self.container_text_view().zoom_level() == ZoomLevel::Everything,
            _ => false,
        }
    }
}

/// `sectionHeader(_:)`: `NSMenuItem.sectionHeader(title:)`. Swift falls back
/// to a disabled item before macOS 14; Upleft, like Downright's package,
/// requires macOS 14, so the fallback is unreachable on both.
fn section_header(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), mtm)
}

fn animations(items: &[&CABasicAnimation]) -> Retained<NSArray<CAAnimation>> {
    let animations: Vec<Retained<CAAnimation>> =
        items.iter().map(|animation| Retained::into_super(Retained::into_super(animation.retain()))).collect();
    NSArray::from_retained_slice(&animations)
}
