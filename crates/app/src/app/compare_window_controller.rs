//! Port of `App/CompareWindowController.swift`: compare any two files, or two
//! versions of one file, as a **rendered** diff (§9.3) — not two columns of
//! `+`/`-` source lines. Changed blocks get a margin bar and changed words are
//! highlighted inside the prose on the side they belong to, which is the same
//! treatment §8.1 gives an external write.
//!
//! `CompareWindowController` is a `define_class!` `NSWindowController`
//! subclass of that name and its toolbar's delegate.
//!
//! Swift's initialisers read, parse and diff on the main thread; so do
//! [`CompareWindowController::with_urls`] and [`CompareWindowController::new`].
//! [`CompareWindowController::load`] is the same construction with the file
//! reads, both parses and both diffs done on a worker first (AGENTS.md: never
//! block the main thread); every AppKit call is the same, in the same order.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use block2::RcBlock;
use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBezelStyle, NSButton, NSButtonType, NSColor, NSControlStateValueOn, NSFont,
    NSFontWeightMedium, NSImage, NSLayoutConstraint, NSResponder, NSTextField, NSTextStorage, NSToolbar,
    NSToolbarDelegate, NSToolbarFlexibleSpaceItemIdentifier, NSToolbarItem, NSToolbarItemIdentifier, NSView,
    NSViewBoundsDidChangeNotification, NSWindow, NSWindowController, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSOperationQueue, NSPoint, NSString};
use upleft_core::contracts::{ChangeHunk, ChangeKind};
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_core::text_diff::TextDiff;
use upleft_core::{DirtySet, ParsedDocument};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{RectExt, ns_range, rect};
use upleft_render::render_contracts::RenderMode;
use upleft_render::swift_compat::{smax, smin};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::ChangeMark;

use crate::app::themed_split_view::ThemedSplitView;

/// `CompareWindowController.lockItem`.
const LOCK_ITEM: &str = "scrollLock";

/// What `build` needs besides the two texts: the parsed documents and the
/// hunks in each direction, computed ahead of time by
/// [`CompareWindowController::load`].
struct Prepared {
    left_document: Arc<ParsedDocument>,
    right_document: Arc<ParsedDocument>,
    forward: Vec<ChangeHunk>,
    backward: Vec<ChangeHunk>,
}

pub struct CompareWindowControllerIvars {
    left_storage: Retained<NSTextStorage>,
    right_storage: Retained<NSTextStorage>,
    left_container: RefCell<Option<Retained<MarkdownContainerView>>>,
    right_container: RefCell<Option<Retained<MarkdownContainerView>>>,
    scroll_locked: Cell<bool>,
    is_syncing_scroll: Cell<bool>,
    style_sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    // SAFETY: `initWithWindow:` is forwarded in `init` after the ivars are
    // set; the action and delegate methods keep AppKit's signatures.
    #[unsafe(super(NSWindowController, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "CompareWindowController"]
    #[ivars = CompareWindowControllerIvars]
    pub struct CompareWindowController;

    unsafe impl NSObjectProtocol for CompareWindowController {}

    impl CompareWindowController {
        #[unsafe(method(toggleScrollLock:))]
        fn toggle_scroll_lock(&self, sender: &NSButton) {
            self.ivars().scroll_locked.set(sender.state() == NSControlStateValueOn);
        }
    }

    unsafe impl NSToolbarDelegate for CompareWindowController {
        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn toolbar_default_item_identifiers(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            item_identifiers()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn toolbar_allowed_item_identifiers(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            item_identifiers()
        }

        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(
            &self,
            _toolbar: &NSToolbar,
            identifier: &NSToolbarItemIdentifier,
            _flag: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            self.toolbar_item_for(identifier)
        }
    }
);

/// `[.flexibleSpace, Self.lockItem]`.
fn item_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
    // SAFETY: AppKit's identifier constant.
    let flexible = unsafe { NSToolbarFlexibleSpaceItemIdentifier };
    NSArray::from_slice(&[flexible, &*NSString::from_str(LOCK_ITEM)])
}

/// `DocumentIO.read(contentsOf:).text`, or `""` on failure (`try?` … `?? ""`).
fn read_text(url: &FileUrl) -> String {
    DocumentIO::read(Path::new(&url.path())).map(|(text, _)| text).unwrap_or_default()
}

impl CompareWindowController {
    /// `convenience init(left:right:)`: reads both files on the calling (main)
    /// thread, as Swift does.
    pub fn with_urls(left: &FileUrl, right: &FileUrl, mtm: MainThreadMarker) -> Retained<CompareWindowController> {
        let left_text = read_text(left);
        let right_text = read_text(right);
        Self::new(
            &left_text,
            &left.last_path_component(),
            &right_text,
            &right.last_path_component(),
            Some(left),
            mtm,
        )
    }

    /// `init(leftText:leftTitle:rightText:rightTitle:documentURL:)`. Swift
    /// accepts `documentURL` and never reads it; so does the port.
    pub fn new(
        left_text: &str,
        left_title: &str,
        right_text: &str,
        right_title: &str,
        document_url: Option<&FileUrl>,
        mtm: MainThreadMarker,
    ) -> Retained<CompareWindowController> {
        Self::init(left_text, left_title, right_text, right_title, document_url, None, mtm)
    }

    /// [`with_urls`](Self::with_urls) without blocking the main thread: the
    /// reads, both parses and both diffs run on a user-initiated global
    /// queue, then the controller is built on the main queue and handed to
    /// `completion`.
    pub fn load(
        left: FileUrl,
        right: FileUrl,
        completion: impl FnOnce(Retained<CompareWindowController>) + 'static,
        mtm: MainThreadMarker,
    ) {
        let completion =
            MainThreadBound::new(Box::new(completion) as Box<dyn FnOnce(Retained<CompareWindowController>)>, mtm);
        DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated)).exec_async(
            move || {
                let left_text = read_text(&left);
                let right_text = read_text(&right);
                let prepared = Prepared {
                    left_document: MarkdownParser::parse(&left_text),
                    right_document: MarkdownParser::parse(&right_text),
                    forward: TextDiff::hunks(&left_text, &right_text),
                    backward: TextDiff::hunks(&right_text, &left_text),
                };
                DispatchQueue::main().exec_async(move || {
                    let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                    let controller = Self::init(
                        &left_text,
                        &left.last_path_component(),
                        &right_text,
                        &right.last_path_component(),
                        Some(&left),
                        Some(prepared),
                        mtm,
                    );
                    (completion.into_inner(mtm))(controller);
                });
            },
        );
    }

    fn init(
        left_text: &str,
        left_title: &str,
        right_text: &str,
        right_title: &str,
        _document_url: Option<&FileUrl>,
        prepared: Option<Prepared>,
        mtm: MainThreadMarker,
    ) -> Retained<CompareWindowController> {
        let left_storage = NSTextStorage::new();
        let right_storage = NSTextStorage::new();
        let style_sheet = StyleSheet::new(
            ThemeStore::shared().current(),
            &NSApplication::sharedApplication(mtm).effectiveAppearance(),
            None,
        );

        // SAFETY: a plain titled window; its controller owns it (AppKit
        // ignores `releasedWhenClosed` for windows owned by a controller).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 1180.0, 760.0),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(&format!("{left_title} ⟷ {right_title}")));

        let this = Self::alloc(mtm).set_ivars(CompareWindowControllerIvars {
            left_storage,
            right_storage,
            left_container: RefCell::new(None),
            right_container: RefCell::new(None),
            scroll_locked: Cell::new(true),
            is_syncing_scroll: Cell::new(false),
            style_sheet: RefCell::new(Rc::new(style_sheet)),
        });
        let this: Retained<CompareWindowController> = unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };
        this.build(left_text, left_title, right_text, right_title, prepared, mtm);
        this
    }

    fn build(
        &self,
        left_text: &str,
        left_title: &str,
        right_text: &str,
        right_title: &str,
        prepared: Option<Prepared>,
        mtm: MainThreadMarker,
    ) {
        let ivars = self.ivars();
        ivars.left_storage.replaceCharactersInRange_withString(ns_range(0, 0), &NSString::from_str(left_text));
        ivars.right_storage.replaceCharactersInRange_withString(ns_range(0, 0), &NSString::from_str(right_text));

        let left_container = MarkdownContainerView::with_storage(&ivars.left_storage, mtm);
        let right_container = MarkdownContainerView::with_storage(&ivars.right_storage, mtm);
        *ivars.left_container.borrow_mut() = Some(left_container.clone());
        *ivars.right_container.borrow_mut() = Some(right_container.clone());

        let (left_document, right_document, precomputed) = match prepared {
            Some(prepared) => (
                prepared.left_document,
                prepared.right_document,
                Some((prepared.forward, prepared.backward)),
            ),
            None => (MarkdownParser::parse(left_text), MarkdownParser::parse(right_text), None),
        };

        let style_sheet = ivars.style_sheet.borrow().clone();
        for (container, document) in [(&left_container, left_document), (&right_container, right_document)] {
            container.text_view().set_style_sheet(style_sheet.clone());
            container.text_view().set_mode(RenderMode::Read);
            container.text_view().update(document, &DirtySet::wholesale(), true);
        }

        let left_units: Vec<u16> = left_text.encode_utf16().collect();
        let right_units: Vec<u16> = right_text.encode_utf16().collect();
        let (forward, backward) = match precomputed {
            Some((forward, backward)) => (Some(forward), Some(backward)),
            None => (None, None),
        };

        // Diff each direction so deletions mark up on the left and insertions
        // on the right, rather than both sides showing the same hunk list.
        let forward = forward.unwrap_or_else(|| TextDiff::hunks(left_text, right_text));
        right_container.text_view().set_change_marks(change_marks(&forward, right_units.len(), &left_units));
        let backward = backward.unwrap_or_else(|| TextDiff::hunks(right_text, left_text));
        left_container.text_view().set_change_marks(change_marks(&backward, left_units.len(), &right_units));

        let split = ThemedSplitView::new(style_sheet, true, mtm);
        split.setTranslatesAutoresizingMaskIntoConstraints(false);
        split.addArrangedSubview(&pane(left_title, &left_container, mtm));
        split.addArrangedSubview(&pane(right_title, &right_container, mtm));

        let root = NSView::new(mtm);
        root.addSubview(&split);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            split.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
            split.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
            split.topAnchor().constraintEqualToAnchor(&root.topAnchor()),
            split.bottomAnchor().constraintEqualToAnchor(&root.bottomAnchor()),
        ]));
        if let Some(window) = self.window() {
            window.setContentView(Some(&root));
        }

        self.observe_scroll(&left_container, &right_container);
        self.observe_scroll(&right_container, &left_container);

        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("CompareToolbar"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(self)));
        if let Some(window) = self.window() {
            window.setToolbar(Some(&toolbar));
        }
    }

    /// Scroll-locked by default: comparing two documents means reading the
    /// same place in both. Locking on *fraction* rather than on point offset
    /// keeps the panes together even when one side is substantially longer.
    fn observe_scroll(&self, source: &MarkdownContainerView, mirror: &MarkdownContainerView) {
        let clip = source.scroll_view().contentView();
        clip.setPostsBoundsChangedNotifications(true);
        let weak_self: ObjcWeak<CompareWindowController> = ObjcWeak::from(self);
        let weak_source: ObjcWeak<MarkdownContainerView> = ObjcWeak::from(source);
        let weak_mirror: ObjcWeak<MarkdownContainerView> = ObjcWeak::from(mirror);
        let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
            let Some(this) = weak_self.load() else { return };
            let ivars = this.ivars();
            if !ivars.scroll_locked.get() || ivars.is_syncing_scroll.get() {
                return;
            }
            let (Some(source), Some(mirror)) = (weak_source.load(), weak_mirror.load()) else { return };
            ivars.is_syncing_scroll.set(true);

            let source_scroll = source.scroll_view();
            let source_height = smax(
                1.0,
                source_scroll.documentView().map_or(1.0, |view| view.bounds().height())
                    - source_scroll.contentView().bounds().height(),
            );
            let fraction = smin(1.0, smax(0.0, source_scroll.contentView().bounds().origin.y / source_height));
            let mirror_scroll = mirror.scroll_view();
            let mirror_height = smax(
                1.0,
                mirror_scroll.documentView().map_or(1.0, |view| view.bounds().height())
                    - mirror_scroll.contentView().bounds().height(),
            );
            mirror_scroll.contentView().scrollToPoint(NSPoint::new(0.0, fraction * mirror_height));
            mirror_scroll.reflectScrolledClipView(&mirror_scroll.contentView());

            ivars.is_syncing_scroll.set(false);
        });
        // Swift keeps no token and its `deinit` removes only selector
        // observers, so the block observer stays registered for the life of
        // the process (its weak captures make it a no-op once the controller
        // is gone). The port does the same.
        // SAFETY: the block runs on the main queue (`queue: .main`), where
        // the weak references are loaded.
        let _ = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
    }

    /// `toolbar(_:itemForItemIdentifier:willBeInsertedIntoToolbar:)`.
    fn toolbar_item_for(&self, identifier: &NSToolbarItemIdentifier) -> Option<Retained<NSToolbarItem>> {
        if identifier.to_string() != LOCK_ITEM {
            return None;
        }
        let mtm = self.mtm();
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("link"),
            Some(&NSString::from_str("Scroll lock")),
        )
        .unwrap_or_else(|| NSImage::new());
        // SAFETY: `self` implements `toggleScrollLock:` and outlives its
        // toolbar.
        let button =
            unsafe { NSButton::buttonWithImage_target_action(&image, Some(self), Some(sel!(toggleScrollLock:)), mtm) };
        button.setButtonType(NSButtonType::PushOnPushOff);
        button.setState(NSControlStateValueOn);
        // Swift's `.texturedRounded`, which the SDK now spells
        // `.toolbar` (same value).
        button.setBezelStyle(NSBezelStyle::Toolbar);
        let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), identifier);
        item.setView(Some(&button));
        item.setLabel(&NSString::from_str("Scroll Lock"));
        Some(item)
    }

    /// Whether the panes scroll together (the toolbar's Scroll Lock).
    pub fn scroll_locked(&self) -> bool {
        self.ivars().scroll_locked.get()
    }

    /// The left pane.
    pub fn left_container(&self) -> Option<Retained<MarkdownContainerView>> {
        self.ivars().left_container.borrow().clone()
    }

    /// The right pane.
    pub fn right_container(&self) -> Option<Retained<MarkdownContainerView>> {
        self.ivars().right_container.borrow().clone()
    }
}

/// `hunks.map { MarkdownTextView.ChangeMark(kind:range:words:deletedText:) }`.
fn change_marks(hunks: &[ChangeHunk], new_length: usize, old_units: &[u16]) -> Vec<ChangeMark> {
    hunks
        .iter()
        .map(|hunk| ChangeMark {
            deleted_text: if hunk.kind == ChangeKind::Deleted {
                // `(oldText as NSString).substring(with: oldRange)`.
                String::from_utf16_lossy(&old_units[hunk.old_range.as_usize_range()])
            } else {
                String::new()
            },
            ..ChangeMark::new(
                hunk.kind,
                TextDiff::anchor_range(hunk, new_length as isize),
                hunk.word_ranges.clone(),
            )
        })
        .collect()
}

/// `pane(titled:content:)`.
fn pane(title: &str, content: &NSView, mtm: MainThreadMarker) -> Retained<NSView> {
    let header = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    header.setFont(Some(&NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
    header.setTextColor(Some(&NSColor::secondaryLabelColor()));
    header.setTranslatesAutoresizingMaskIntoConstraints(false);

    let pane = NSView::new(mtm);
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    pane.addSubview(&header);
    pane.addSubview(content);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        header.leadingAnchor().constraintEqualToAnchor_constant(&pane.leadingAnchor(), 12.0),
        header.topAnchor().constraintEqualToAnchor_constant(&pane.topAnchor(), 6.0),
        content.leadingAnchor().constraintEqualToAnchor(&pane.leadingAnchor()),
        content.trailingAnchor().constraintEqualToAnchor(&pane.trailingAnchor()),
        content.topAnchor().constraintEqualToAnchor_constant(&header.bottomAnchor(), 6.0),
        content.bottomAnchor().constraintEqualToAnchor(&pane.bottomAnchor()),
    ]));
    pane
}
