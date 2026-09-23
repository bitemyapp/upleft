//! Port of `App/VersionTimelineWindowController.swift`: the version timeline
//! (§8.3, `⌘⇧V`).
//!
//! A separate window rather than an in-window overlay (§15 Q6): scrubbing
//! through a month of an agent's rewrites is a *comparison* activity, and the
//! document you are comparing against needs to stay visible next to it.
//!
//! `VersionTimelineWindowController` is a `define_class!`
//! `NSWindowController` subclass of that name. The timeline view holds its
//! delegate weakly as a Rust trait object, so the controller owns a small
//! proxy ([`VersionTimelineWindowControllerDelegate`]) that implements
//! `VersionTimelineDelegate` and forwards here.
//!
//! Main-thread I/O, as in Swift: the snapshot index and the shown versions'
//! objects are read on the main thread.

use std::cell::{OnceCell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSBackingStoreType, NSColor, NSResponder, NSTextAlignment, NSTextField,
    NSTextStorage, NSView, NSWindow, NSWindowController, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDateFormatter, NSLocale, NSString};
use upleft_core::contracts::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_core::text_diff::TextDiff;
use upleft_foundation::date::Date;
use upleft_render::appkit_compat::rect;
use upleft_render::core_types::ChangeKind;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::ChangeMark;
use upleft_swift_text::ns::{NSStringExt, utf16};

use crate::ai::markdown_document::MarkdownDocument;
use crate::ai::snapshot_store::{SnapshotStore, VersionRecord};
use crate::panels::appkit_support::activate;
use crate::panels::version_timeline_view::{VersionTimelineDelegate, VersionTimelineView};

/// The timeline's delegate: `self` in Swift.
pub struct VersionTimelineWindowControllerDelegate {
    controller: ObjcWeak<VersionTimelineWindowController>,
}

impl VersionTimelineWindowControllerDelegate {
    pub fn controller(&self) -> Option<Retained<VersionTimelineWindowController>> {
        self.controller.load()
    }
}

pub struct VersionTimelineWindowControllerIvars {
    markdown_document: Retained<MarkdownDocument>,
    preview_storage: Retained<NSTextStorage>,
    preview: RefCell<Option<Retained<MarkdownContainerView>>>,
    timeline: Retained<VersionTimelineView>,
    versions: RefCell<Vec<VersionRecord>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    delegate: OnceCell<Rc<VersionTimelineWindowControllerDelegate>>,
}

define_class!(
    /// `VersionTimelineWindowController`.
    // SAFETY: `initWithWindow:` is forwarded in `new` after the ivars are
    // set; the class overrides nothing.
    #[unsafe(super(NSWindowController, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "VersionTimelineWindowController"]
    #[ivars = VersionTimelineWindowControllerIvars]
    pub struct VersionTimelineWindowController;

    unsafe impl NSObjectProtocol for VersionTimelineWindowController {}
);

impl VersionTimelineWindowController {
    /// `init(document:styleSheet:)`.
    pub fn new(
        document: &MarkdownDocument,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<VersionTimelineWindowController> {
        // Stored-property initial values, in declaration order.
        let markdown_document: Retained<MarkdownDocument> = document.retain();
        let preview_storage = NSTextStorage::new();
        let timeline = VersionTimelineView::new_current(mtm);
        // SAFETY: a plain titled window; its controller owns it (AppKit
        // ignores `releasedWhenClosed` for windows owned by a controller).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 880.0, 720.0),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(&format!("History \u{2014} {}", markdown_document.display_name())));
        let this = Self::alloc(mtm).set_ivars(VersionTimelineWindowControllerIvars {
            markdown_document,
            preview_storage,
            preview: RefCell::new(None),
            timeline,
            versions: RefCell::new(Vec::new()),
            style_sheet: RefCell::new(style_sheet),
            delegate: OnceCell::new(),
        });
        // SAFETY: `NSWindowController`'s designated initialiser.
        let this: Retained<VersionTimelineWindowController> =
            unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };
        let _ = this
            .ivars()
            .delegate
            .set(Rc::new(VersionTimelineWindowControllerDelegate { controller: ObjcWeak::from(&*this) }));
        this.build(mtm);
        this
    }

    /// The timeline, for tests (`timeline` is private in Swift).
    pub fn timeline_for_testing(&self) -> Retained<VersionTimelineView> {
        self.ivars().timeline.clone()
    }

    /// The preview pane, for tests.
    pub fn preview_for_testing(&self) -> Option<Retained<MarkdownContainerView>> {
        self.ivars().preview.borrow().clone()
    }

    fn preview(&self) -> Retained<MarkdownContainerView> {
        self.ivars().preview.borrow().clone().expect("preview is set by build")
    }

    /// `build()`.
    fn build(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        *ivars.versions.borrow_mut() = ivars.markdown_document.versions();
        let preview = MarkdownContainerView::with_storage(&ivars.preview_storage, mtm);
        *ivars.preview.borrow_mut() = Some(preview.clone());
        let style_sheet = ivars.style_sheet.borrow().clone();
        preview.text_view().set_style_sheet(style_sheet.clone());
        preview.text_view().set_mode(RenderMode::Read);
        preview.setTranslatesAutoresizingMaskIntoConstraints(false);

        let timeline = &ivars.timeline;
        timeline.set_style_sheet(style_sheet);
        let versions = ivars.versions.borrow().clone();
        let count = versions.len() as isize;
        timeline.set_versions(versions);
        timeline.set_selected_index(0.max(count - 1));
        let delegate = ivars.delegate.get().expect("the delegate proxy is created in init").clone();
        timeline.set_delegate(Some(Rc::downgrade(&delegate) as Weak<dyn VersionTimelineDelegate>));
        timeline.setTranslatesAutoresizingMaskIntoConstraints(false);

        let root = NSView::new(mtm);
        root.addSubview(&preview);
        root.addSubview(timeline);
        activate(&[
            preview.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
            preview.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
            preview.topAnchor().constraintEqualToAnchor(&root.topAnchor()),
            timeline.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
            timeline.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
            timeline.topAnchor().constraintEqualToAnchor(&preview.bottomAnchor()),
            timeline.bottomAnchor().constraintEqualToAnchor(&root.bottomAnchor()),
            timeline.heightAnchor().constraintEqualToConstant(96.0),
        ]);
        if let Some(window) = self.window() {
            window.setContentView(Some(&root));
        }

        let newest = ivars.versions.borrow().last().cloned();
        if ivars.versions.borrow().is_empty() {
            self.show_empty_state(&root, mtm);
        } else if let Some(newest) = newest {
            self.show(&newest);
        }
    }

    /// `showEmptyState(in:)`.
    fn show_empty_state(&self, root: &NSView, mtm: MainThreadMarker) {
        let label = NSTextField::labelWithString(
            &NSString::from_str(
                "No history yet.\n\nUpleft snapshots this file every time something outside the app rewrites it. \
                 Come back after an agent has touched it.",
            ),
            mtm,
        );
        label.setAlignment(NSTextAlignment::Center);
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.setMaximumNumberOfLines(0);
        root.addSubview(&label);
        activate(&[
            label.centerXAnchor().constraintEqualToAnchor(&root.centerXAnchor()),
            label.centerYAnchor().constraintEqualToAnchor(&root.centerYAnchor()),
            label.widthAnchor().constraintLessThanOrEqualToConstant(380.0),
        ]);
    }

    /// `show(_:)`: renders one version, highlighting what changed relative
    /// to the version immediately before it, so scrubbing shows changes
    /// *between steps* rather than against the current buffer.
    fn show(&self, record: &VersionRecord) {
        let Some(text) = SnapshotStore::shared().text(record) else { return };
        let ivars = self.ivars();
        let length = ivars.preview_storage.length();
        ivars
            .preview_storage
            .replaceCharactersInRange_withString(objc2_foundation::NSRange::new(0, length), &NSString::from_str(&text));
        let parsed = MarkdownParser::parse(&text);
        let preview = self.preview();
        preview.text_view().update(parsed, &DirtySet::wholesale(), true);

        let index = ivars.versions.borrow().iter().position(|version| version == record);
        let previous = match index {
            Some(index) if index > 0 => {
                let prior = ivars.versions.borrow()[index - 1].clone();
                SnapshotStore::shared().text(&prior)
            }
            _ => None,
        };
        let Some(previous) = previous else {
            preview.text_view().set_change_marks(Vec::new());
            return;
        };
        let new_length = utf16(&text).len() as isize;
        let previous_units = utf16(&previous);
        let marks: Vec<ChangeMark> = TextDiff::hunks(&previous, &text)
            .iter()
            .map(|hunk| {
                let mut mark =
                    ChangeMark::new(hunk.kind, TextDiff::anchor_range(hunk, new_length), hunk.word_ranges.clone());
                mark.deleted_text = if hunk.kind == ChangeKind::Deleted {
                    previous_units.as_slice().substring(hunk.old_range)
                } else {
                    String::new()
                };
                mark
            })
            .collect();
        preview.text_view().set_change_marks(marks);
    }

    /// `versionTimeline(_:didScrubTo:)`.
    pub fn version_timeline_did_scrub_to(&self, _view: &VersionTimelineView, record: &VersionRecord) {
        self.show(record);
    }

    /// `versionTimeline(_:didRequestRestore:)`.
    pub fn version_timeline_did_request_restore(&self, _view: &VersionTimelineView, record: &VersionRecord) {
        let alert = NSAlert::new(MainThreadMarker::from(self));
        alert.setMessageText(&NSString::from_str("Restore this version?"));
        alert.setInformativeText(&NSString::from_str(&format!(
            "The current text is replaced with the version from {}. This is an ordinary edit \u{2014} \u{2318}Z undoes it.",
            formatted_abbreviated_shortened(record.date)
        )));
        alert.addButtonWithTitle(&NSString::from_str("Restore"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        if alert.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        self.ivars().markdown_document.restore(record);
        self.close();
    }
}

/// `date.formatted(date: .abbreviated, time: .shortened)`.
///
/// Foundation's `Date.FormatStyle` has no Objective-C face. Its pattern is
/// the current locale's best pattern for the skeleton `yMMMdjmm`, with the
/// hour field kept at the skeleton's length (ICU's
/// `UDATPG_MATCH_HOUR_FIELD_LENGTH`), which `NSDateFormatter` templates do
/// not do. A Swift probe matched this on 24 locales × 400 dates.
pub(crate) fn formatted_abbreviated_shortened(date: Date) -> String {
    let locale = NSLocale::currentLocale();
    let template = NSString::from_str("yMMMdjmm");
    let Some(pattern) = NSDateFormatter::dateFormatFromTemplate_options_locale(&template, 0, Some(&locale)) else {
        return String::new();
    };
    let formatter = NSDateFormatter::new();
    formatter.setDateFormat(Some(&NSString::from_str(&single_hour_field(&pattern.to_string()))));
    let date = NSDate::dateWithTimeIntervalSinceReferenceDate(date.time_interval_since_reference_date);
    formatter.stringFromDate(&date).to_string()
}

/// Shortens every unquoted run of an hour field (`H`, `h`, `K`, `k`) to one
/// letter.
fn single_hour_field(pattern: &str) -> String {
    let characters: Vec<char> = pattern.chars().collect();
    let mut output = String::with_capacity(pattern.len());
    let mut in_quote = false;
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character == '\'' {
            in_quote = !in_quote;
            output.push(character);
            index += 1;
            continue;
        }
        if !in_quote && matches!(character, 'H' | 'h' | 'K' | 'k') {
            let mut end = index;
            while end < characters.len() && characters[end] == character {
                end += 1;
            }
            output.push(character);
            index = end;
            continue;
        }
        output.push(character);
        index += 1;
    }
    output
}

// MARK: - VersionTimelineDelegate

impl VersionTimelineDelegate for VersionTimelineWindowControllerDelegate {
    fn version_timeline_did_scrub_to(&self, view: &VersionTimelineView, record: &VersionRecord) {
        if let Some(controller) = self.controller() {
            controller.version_timeline_did_scrub_to(view, record);
        }
    }

    fn version_timeline_did_request_restore(&self, view: &VersionTimelineView, record: &VersionRecord) {
        if let Some(controller) = self.controller() {
            controller.version_timeline_did_request_restore(view, record);
        }
    }
}
