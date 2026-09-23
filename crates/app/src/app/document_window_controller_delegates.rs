//! Port of `App/DocumentWindowController+Delegates.swift`: the controller as
//! the delegate of its text surface and panels, the §7.1 context menus, and
//! `MarkdownLinkDestination`.
//!
//! Each Swift conformance (`MarkdownTextViewDelegate`, `TaskPanelDelegate`,
//! `DensityGutterDelegate`, `BreadcrumbDelegate`, `FindBarDelegate`,
//! `ConflictBarDelegate`, `ChangeSummaryBarDelegate`, `TidySheetDelegate`,
//! `SearchResultsDelegate`, `HistoryInspectorViewDelegate`) is a trait impl on
//! the controller's delegate proxy ([`DocumentWindowControllerDelegates`]).
//! Every trait method forwards to an inherent `DocumentWindowController`
//! method named after the Swift method (`markdownTextView(_:didScroll…)` →
//! `markdown_text_view_did_scroll`, `taskPanel(_:didToggleTaskAt:)` →
//! `task_panel_did_toggle_task_at`), which holds the Swift body; while the
//! controller is gone, the proxy answers with the protocol's defaults.
//!
//! Two `MarkdownTextViewDelegate` methods have their bodies in other Swift
//! files and are forwarded there: `canAcceptDrop`/`didAcceptDrop`
//! (`+AssetInsertion`, `markdown_text_view_can_accept_drop` /
//! `markdown_text_view_did_accept_drop`) and `wantsQuickLookFor`
//! (`Panels/DocumentQuickLook.swift`,
//! `QuickLookHost::markdown_text_view_wants_quick_look_for`).
//!
//! `BlockActionTarget` is a `define_class!` type of that Objective-C name.

use std::cell::RefCell;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem, NSPasteboard,
    NSPasteboardTypeString, NSWorkspace,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSFileManager, NSString, NSURL};
use upleft_core::restructure::Restructure;
use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::swift_text::ns::foundation;
use upleft_core::{NSRange, PathToken, TableAlignment, TextEdit};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::RectExt;
use upleft_render::render_contracts::{RenderMode, SourceFocus};
use upleft_render::view::density_gutter_view::{DensityGutterDelegate, DensityGutterView};
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::{
    ContextTarget, ContextTargetKind, DocumentDrop, MarkdownTextViewDelegate, ScrollPosition,
};
use upleft_swift_text as swift_text;

use crate::ai::markdown_document::SaveIntent;
use crate::ai::snapshot_store::VersionRecord;
use crate::app::app_delegate::DocumentOpenDisposition;
use crate::app::compare_window_controller::CompareWindowController;
use crate::app::document_types;
use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_commands::app_delegate;
use crate::app::document_window_controller_support::CopyFlavour;
use crate::app::main_menu::MainMenu;
use crate::assets::asset_resolver::url_with_string;
use crate::panels::breadcrumb_view::{BreadcrumbDelegate, BreadcrumbView};
use crate::panels::change_summary_bar_view::{ChangeSummaryBarDelegate, ChangeSummaryBarView};
use crate::panels::conflict_bar_view::{ConflictBarDelegate, ConflictBarView};
use crate::panels::document_quick_look::QuickLookHost;
use crate::panels::find_bar_view::{FindBarDelegate, FindBarView};
use crate::panels::history_inspector_view::{HistoryInspectorView, HistoryInspectorViewDelegate};
use crate::panels::search_results_panel_view::{SearchResultsDelegate, SearchResultsPanelView};
use crate::panels::task_panel_view::{TaskPanelDelegate, TaskPanelView};
use crate::panels::tidy_sheet_view::{TidySheetDelegate, TidySheetView};
use crate::review::review_sidecar::ReviewKind;
use crate::security::document_trust::TrustEffect;
use crate::support::commands::Command;
use crate::support::find_engine::{FindEngine, FindQuery, SiblingHit};
use crate::support::preferences::Preferences;

// MARK: - MarkdownLinkDestination

/// `enum MarkdownLinkDestination: Equatable`. The URL payloads are Swift
/// `URL(string:)` values, which are `-[NSURL URLWithString:]` here (see
/// `assets::asset_resolver`); equality is `NSURL`'s `isEqual:`, which is what
/// Swift's `URL ==` compares.
#[derive(Clone, Debug)]
pub enum MarkdownLinkDestination {
    Anchor(String),
    Web(Retained<NSURL>),
    LocalFile(Retained<NSURL>),
    Automation(Retained<NSURL>),
    Relative(String),
    Invalid,
}

impl PartialEq for MarkdownLinkDestination {
    fn eq(&self, other: &Self) -> bool {
        use MarkdownLinkDestination::*;
        match (self, other) {
            (Anchor(a), Anchor(b)) | (Relative(a), Relative(b)) => swift_text::str_eq(a, b),
            (Web(a), Web(b)) | (LocalFile(a), LocalFile(b)) | (Automation(a), Automation(b)) => a.isEqual(Some(b)),
            (Invalid, Invalid) => true,
            _ => false,
        }
    }
}

impl MarkdownLinkDestination {
    /// `classify(_:)`.
    pub fn classify(destination: &str) -> MarkdownLinkDestination {
        if swift_text::has_prefix(destination, "#") {
            return MarkdownLinkDestination::Anchor(swift_text::drop_first(destination, 1).to_owned());
        }
        let url = url_with_string(destination);
        let scheme =
            url.as_ref().and_then(|url| url.scheme()).map(|scheme| swift_text::lowercased(&foundation::to_string(&scheme)));
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

// MARK: - Foundation conveniences

/// `url.standardizedFileURL` for a `file:` URL, as a [`FileUrl`]; `None`
/// where the URL has no path (Swift keeps a URL that no file exists at).
fn standardized_file_url(url: &NSURL) -> Option<FileUrl> {
    let path = url.path().map(|path| foundation::to_string(&path)).unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    Some(FileUrl::from_path_is_directory(&path, url.hasDirectoryPath()).standardized_file_url())
}

/// `FileManager.default.fileExists(atPath:)`.
fn file_exists(path: &str) -> bool {
    NSFileManager::defaultManager().fileExistsAtPath(&NSString::from_str(path))
}

/// `NSWorkspace.shared.open(url)`.
fn workspace_open(url: &NSURL) {
    NSWorkspace::sharedWorkspace().openURL(url);
}

/// `NSWorkspace.shared.selectFile(path, inFileViewerRootedAtPath: root)`.
fn workspace_select_file(path: Option<&str>, root: &str) {
    NSWorkspace::sharedWorkspace()
        .selectFile_inFileViewerRootedAtPath(path.map(NSString::from_str).as_deref(), &NSString::from_str(root));
}

/// `NSWorkspace.shared.activateFileViewerSelecting([url])`.
fn workspace_reveal(url: &FileUrl) {
    let urls = NSArray::from_retained_slice(&[url.to_nsurl()]);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
}

/// `NSPasteboard.general.clearContents()` then `setString(_:forType: .string)`.
fn copy_string_to_pasteboard(string: &str) {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: AppKit's immutable pasteboard type constant.
    pasteboard.setString_forType(&NSString::from_str(string), unsafe { NSPasteboardTypeString });
}

// MARK: - Text surface

impl MarkdownTextViewDelegate for DocumentWindowControllerDelegates {
    fn did_activate_link(&self, view: &MarkdownTextView, destination: &str, range: NSRange, modifiers: NSEventModifierFlags) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_activate_link(view, destination, range, modifiers);
        }
    }

    fn did_activate_image(&self, view: &MarkdownTextView, source: &str, range: NSRange) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_activate_image(view, source, range);
        }
    }

    fn did_activate_front_matter_at(&self, view: &MarkdownTextView, range: NSRange) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_activate_front_matter_at(view, range);
        }
    }

    fn did_activate_path_token(&self, view: &MarkdownTextView, token: &PathToken, range: NSRange) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_activate_path_token(view, token, range);
        }
    }

    fn did_toggle_checkbox_at_mark_offset(&self, view: &MarkdownTextView, offset: isize) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_toggle_checkbox_at_mark_offset(view, offset);
        }
    }

    fn did_activate_heading_anchor(&self, view: &MarkdownTextView, heading_index: usize, modifiers: NSEventModifierFlags) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_activate_heading_anchor(view, heading_index, modifiers);
        }
    }

    fn did_request_heading_level(&self, view: &MarkdownTextView, level: Option<isize>, heading_index: usize) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_request_heading_level(view, level, heading_index);
        }
    }

    fn wants_context_menu_for(&self, view: &MarkdownTextView, target: &ContextTarget) -> Option<Retained<NSMenu>> {
        self.controller()?.markdown_text_view_wants_context_menu_for(view, target)
    }

    fn path_exists_for(&self, view: &MarkdownTextView, token: &PathToken) -> bool {
        match self.controller() {
            Some(controller) => controller.markdown_text_view_path_exists_for(view, token),
            None => true,
        }
    }

    fn did_change_selection(&self, view: &MarkdownTextView) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_change_selection(view);
        }
    }

    fn did_scroll(&self, view: &MarkdownTextView) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_scroll(view);
        }
    }

    fn did_change_source_focus(&self, view: &MarkdownTextView, focus: SourceFocus) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_change_source_focus(view, focus);
        }
    }

    fn did_edit(&self, view: &MarkdownTextView, range: NSRange, delta: isize) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_edit(view, range, delta);
        }
    }

    fn did_navigate_to(&self, view: &MarkdownTextView, destination: isize) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_navigate_to(view, destination);
        }
    }

    fn did_request_text_size_steps(&self, view: &MarkdownTextView, steps: isize) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_request_text_size_steps(view, steps);
        }
    }

    fn did_request_smart_text_zoom(&self, view: &MarkdownTextView) {
        if let Some(controller) = self.controller() {
            controller.markdown_text_view_did_request_smart_text_zoom(view);
        }
    }

    fn should_claim_scroll_gesture(&self, view: &MarkdownTextView, event: &NSEvent) -> bool {
        self.controller().is_some_and(|controller| controller.markdown_text_view_should_claim_scroll_gesture(view, event))
    }

    /// `+AssetInsertion`.
    fn can_accept_drop(&self, view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        self.controller().is_some_and(|controller| controller.markdown_text_view_can_accept_drop(view, drop))
    }

    /// `+AssetInsertion`.
    fn did_accept_drop(&self, view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        self.controller().is_some_and(|controller| controller.markdown_text_view_did_accept_drop(view, drop))
    }

    /// `Panels/DocumentQuickLook.swift`.
    fn wants_quick_look_for(&self, view: &MarkdownTextView, target: &ContextTarget) -> bool {
        self.controller().is_some_and(|controller| {
            QuickLookHost::markdown_text_view_wants_quick_look_for(&*controller, view, target)
        })
    }
}

impl DocumentWindowController {
    fn delegates_mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    /// `markdownTextView(_:didRequestTextSizeSteps:)`.
    pub fn markdown_text_view_did_request_text_size_steps(&self, _view: &MarkdownTextView, steps: isize) {
        if steps == 0 {
            return;
        }
        self.adjust_text_size(steps as CGFloat);
    }

    /// `markdownTextViewDidRequestSmartTextZoom(_:)`.
    pub fn markdown_text_view_did_request_smart_text_zoom(&self, _view: &MarkdownTextView) {
        Preferences::shared().update(|values| {
            values.text_size_adjustment = if values.text_size_adjustment == 0.0 { 3.0 } else { 0.0 };
        });
    }

    /// Four things can happen on this surface's wheel: ⌘ sizes the text, ⌥
    /// steps structural detail, ⇧ and two fingers sideways move through jump
    /// history, and two bare fingers sideways switch Document↔Source. They
    /// are asked in that order and the first to claim wins; anything none of
    /// them has claimed goes straight back, so vertical scrolling — which is
    /// what almost every one of these events is — is untouched.
    pub fn markdown_text_view_should_claim_scroll_gesture(&self, _view: &MarkdownTextView, event: &NSEvent) -> bool {
        self.document_scroll_gestures().handle(event)
    }

    /// `markdownTextView(_:didRequestHeadingLevel:headingIndex:)`.
    pub fn markdown_text_view_did_request_heading_level(
        &self,
        _view: &MarkdownTextView,
        level: Option<isize>,
        heading_index: usize,
    ) {
        self.markdown_document().ensure_parsed_current();
        if !(heading_index < self.markdown_document().parsed().headings.len()) {
            return;
        }
        if let Some(level) = level {
            let edits =
                Restructure::set_heading_level(&self.markdown_document().parsed(), heading_index as isize, level);
            if edits.is_empty() {
                return;
            }
            // The heading chip is a rendered control, not an insertion
            // gesture. Preserve both cameras before mutating the shared
            // storage so the async decoration pass cannot make either pane
            // follow the menu.
            let anchor = self.markdown_document().parsed().headings[heading_index].range.location;
            self.apply_in_place_document_edits(&edits, &format!("Set Heading {level}"), Some(anchor));
            return;
        }
        let edits = Restructure::heading_to_body_text(&self.markdown_document().parsed(), heading_index as isize);
        if edits.is_empty() {
            return;
        }
        let anchor = self.markdown_document().parsed().headings[heading_index].range.location;
        self.apply_in_place_document_edits(&edits, "Body Text", Some(anchor));
    }

    /// `markdownTextView(_:didActivateImage:at:)`.
    pub fn markdown_text_view_did_activate_image(&self, _view: &MarkdownTextView, source: &str, _range: NSRange) {
        self.present_lightbox(source, None);
    }

    /// `markdownTextView(_:didActivateFrontMatterAt:)`.
    pub fn markdown_text_view_did_activate_front_matter_at(&self, _view: &MarkdownTextView, _range: NSRange) {
        self.show_front_matter_editor(None);
    }

    /// `markdownTextView(_:didActivateLink:at:modifiers:)`.
    pub fn markdown_text_view_did_activate_link(
        &self,
        _view: &MarkdownTextView,
        destination: &str,
        _range: NSRange,
        modifiers: NSEventModifierFlags,
    ) {
        let mtm = self.delegates_mtm();
        match MarkdownLinkDestination::classify(destination) {
            MarkdownLinkDestination::Anchor(slug) => {
                let parsed = self.markdown_document().parsed();
                let Some(heading) = parsed.headings.iter().find(|heading| swift_text::str_eq(&heading.slug, &slug))
                else {
                    return;
                };
                self.jump(heading.range.location, &heading.title, true);
            }

            MarkdownLinkDestination::Web(url) => {
                let target = url.clone();
                self.authorize_external_url(&url, move || workspace_open(&target));
            }

            MarkdownLinkDestination::LocalFile(target) => {
                let Some(target) = standardized_file_url(&target) else { return };
                if !file_exists(&target.path()) {
                    return;
                }
                let opened = target.clone();
                self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &target, move || {
                    if document_types::is_markdown(&opened.path_extension()) {
                        if let Some(delegate) = app_delegate(mtm) {
                            delegate.open(&opened, None, None, false, DocumentOpenDisposition::Tab, None);
                        }
                    } else if document_types::executes_when_opened(&opened) {
                        // "Open in editor" never means "run this": an
                        // application bundle or Terminal script linked from a
                        // document is revealed in Finder instead, so showing
                        // where it lives stays possible while running it
                        // stays a manual act.
                        workspace_select_file(Some(&opened.path()), &opened.deleting_last_path_component().path());
                    } else {
                        workspace_open(&opened.to_nsurl());
                    }
                });
            }

            MarkdownLinkDestination::Automation(url) => {
                let target = url.clone();
                self.authorize_automation_url(&url, move || workspace_open(&target));
            }

            MarkdownLinkDestination::Invalid => {}

            MarkdownLinkDestination::Relative(relative) => {
                // A relative link opens in place; ⌘-click keeps the current
                // document and opens the target in the active native tab
                // group.
                let Some(base) = self.markdown_document().url().map(|url| url.deleting_last_path_component()) else {
                    return;
                };
                let target = base.appending_path_component(&relative).standardized_file_url();
                if !file_exists(&target.path()) {
                    return;
                }
                if document_types::is_markdown(&target.path_extension()) {
                    if modifiers.contains(NSEventModifierFlags::Command) {
                        if let Some(delegate) = app_delegate(mtm) {
                            delegate.open(&target, None, None, false, DocumentOpenDisposition::Tab, None);
                        }
                    } else {
                        self.open_in_place(&target);
                    }
                } else if document_types::executes_when_opened(&target) {
                    // Same rule as `.localFile`: reveal execution-capable
                    // targets, never run them from inside a document.
                    let revealed = target.clone();
                    self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &target, move || {
                        workspace_select_file(Some(&revealed.path()), &revealed.deleting_last_path_component().path());
                    });
                } else {
                    let opened = target.clone();
                    self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &target, move || {
                        workspace_open(&opened.to_nsurl());
                    });
                }
            }
        }
    }

    /// `markdownTextView(_:didActivatePathToken:at:)`.
    pub fn markdown_text_view_did_activate_path_token(&self, _view: &MarkdownTextView, token: &PathToken, _range: NSRange) {
        let Some(resolution) = self.path_resolver().map(|resolver| resolver.resolve(token)) else { return };
        if !resolution.exists {
            return;
        }
        let Some(url) = resolution.url else { return };
        if resolution.is_directory {
            let root = url.clone();
            self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, move || {
                workspace_select_file(None, &root.path());
            });
        } else if document_types::is_markdown(&url.path_extension()) {
            if let Some(delegate) = app_delegate(self.delegates_mtm()) {
                delegate.open(&url, None, None, false, DocumentOpenDisposition::Tab, None);
            }
        } else {
            let target = url.clone();
            let line = token.line;
            self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, move || {
                Preferences::shared().values().external_editor.open(&target, line);
            });
        }
    }

    /// `markdownTextView(_:didToggleCheckboxAtMarkOffset:)`: writes the file
    /// immediately (§7.1, §8.5).
    pub fn markdown_text_view_did_toggle_checkbox_at_mark_offset(&self, _view: &MarkdownTextView, offset: isize) {
        self.primary_container().text_view().preserve_viewport_on_next_document_update();
        if let Some(split) = self.split_container() {
            split.text_view().preserve_viewport_on_next_document_update();
        }
        self.markdown_document().toggle_task(offset);
    }

    /// `markdownTextView(_:didActivateHeadingAnchor:modifiers:)`.
    pub fn markdown_text_view_did_activate_heading_anchor(
        &self,
        view: &MarkdownTextView,
        heading_index: usize,
        modifiers: NSEventModifierFlags,
    ) {
        let parsed = self.markdown_document().parsed();
        if !(heading_index < parsed.headings.len()) {
            return;
        }
        let heading = &parsed.headings[heading_index];
        if modifiers.contains(NSEventModifierFlags::Option) {
            // ⌥-click folds the section (§7.1).
            let mut folds = view.folded_heading_slugs();
            if folds.contains(&heading.slug) {
                folds.remove(&heading.slug);
            } else {
                folds.insert(heading.slug.clone());
            }
            self.set_shared_folds(folds, Some(view));
        } else {
            self.copy_section_link(Some(heading_index as isize));
        }
    }

    /// `markdownTextView(_:didNavigateTo:)`: a footnote jump is a navigation
    /// like any other, so Back has to undo it — landing at the bottom of a
    /// long document used to be a one-way trip.
    pub fn markdown_text_view_did_navigate_to(&self, _view: &MarkdownTextView, destination: isize) {
        self.record_jump(destination, "Footnote");
    }

    /// `markdownTextView(_:pathExistsFor:)`.
    pub fn markdown_text_view_path_exists_for(&self, _view: &MarkdownTextView, token: &PathToken) -> bool {
        if !Preferences::shared().values().resolve_path_tokens {
            return true;
        }
        // Decoration is a main-thread operation. A cold cache is neutral
        // until the revision-aware background warm completes and refreshes
        // the view.
        self.path_resolver()
            .and_then(|resolver| resolver.cached_resolution(token))
            .map_or(true, |resolution| resolution.exists)
    }

    /// `markdownTextViewDidChangeSelection(_:)`.
    pub fn markdown_text_view_did_change_selection(&self, view: &MarkdownTextView) {
        if self.is_opening_document() {
            return;
        }
        self.synchronize_panes(view, false, false);
        let selection = view.source_selected_range();
        let mut state = self.markdown_document().state();
        state.selection_location = selection.location;
        self.markdown_document().set_state(state);
        let mut state = self.markdown_document().state();
        state.selection_length = selection.length;
        self.markdown_document().set_state(state);
        self.update_focus_dimming_views();
        if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
            toolbar.validateVisibleItems();
        }
        self.refresh_visual_debugger_if_visible();
        // Push cursor position to the status bar.
        self.status_bar_view().set_cursor_position(Some(view.source_position(selection.location)));
    }

    /// `markdownTextViewDidScroll(_:)`.
    pub fn markdown_text_view_did_scroll(&self, view: &MarkdownTextView) {
        self.synchronize_panes(view, false, false);
        self.update_breadcrumb_and_gutter();
        self.note_visible_change_marks();
        let visible = view.enclosingScrollView().map_or_else(|| view.visibleRect(), |scroll| scroll.documentVisibleRect());
        let parsed = self.markdown_document().parsed();
        let current = self
            .visible_heading_index(view.top_visible_offset())
            .map(|index| &parsed.headings[index as usize]);
        if let Some(current) = current
            && let Some(heading_rect) = view.rect_for_offset(current.range.location)
            && heading_rect.max_y() < visible.min_y() + 1.0
        {
            self.breadcrumb_view().show_current_section();
        } else {
            self.breadcrumb_view().hide_current_section();
        }
        self.update_focus_dimming_views();
    }

    /// `markdownTextView(_:didChangeSourceFocus:)`.
    pub fn markdown_text_view_did_change_source_focus(&self, view: &MarkdownTextView, _focus: SourceFocus) {
        self.synchronize_panes(view, false, false);
        self.set_mode(view.mode().normalized_for_editing());
        let mut state = self.markdown_document().state();
        state.mode = RenderMode::Live;
        self.markdown_document().set_state(state);
        self.refresh_source_focus_toolbar();
        self.primary_container().setNeedsLayout(true);
        if let Some(split) = self.split_container() {
            split.setNeedsLayout(true);
        }
    }

    /// `markdownTextView(_:didEdit:delta:)`: the document owns reparsing;
    /// the storage delegate already scheduled it. Change marks shift here so
    /// they keep pointing at the same text.
    pub fn markdown_text_view_did_edit(&self, view: &MarkdownTextView, range: NSRange, delta: isize) {
        self.markdown_document().changes().adjust(range, delta);
        self.markdown_document().note_mutation(&view.last_mutation_provenance());
        self.schedule_find_refresh();
    }

    // MARK: Context menus (§7.1)

    /// Opens a path token in the user's editor. Returns false — and touches
    /// nothing — when the token does not resolve to an existing file,
    /// matching `didActivatePathToken` and the documented missing-path
    /// contract.
    pub fn open_path_token_in_editor(&self, token: &PathToken) -> bool {
        let Some(resolution) = self.path_resolver().map(|resolver| resolver.resolve(token)) else { return false };
        if !resolution.exists {
            return false;
        }
        let Some(url) = resolution.url else { return false };
        let target = url.clone();
        let line = token.line;
        self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, move || {
            Preferences::shared().values().external_editor.open(&target, line);
        });
        true
    }

    /// Reveals a path token's file in Finder; same missing-path rule as
    /// `open_path_token_in_editor`.
    pub fn reveal_path_token_in_finder(&self, token: &PathToken) -> bool {
        let Some(resolution) = self.path_resolver().map(|resolver| resolver.resolve(token)) else { return false };
        if !resolution.exists {
            return false;
        }
        let Some(url) = resolution.url else { return false };
        let target = url.clone();
        self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, move || workspace_reveal(&target));
        true
    }

    /// `markdownTextView(_:wantsContextMenuFor:)`.
    pub fn markdown_text_view_wants_context_menu_for(
        &self,
        view: &MarkdownTextView,
        target: &ContextTarget,
    ) -> Option<Retained<NSMenu>> {
        let mtm = self.delegates_mtm();
        let menu = NSMenu::new(mtm);
        let separator = || NSMenuItem::separatorItem(mtm);
        let weak = || -> ObjcWeak<DocumentWindowController> { ObjcWeak::new(self) };
        match &target.kind {
            ContextTargetKind::Heading(index) => {
                let range = self.markdown_document().parsed().headings[*index].range;
                menu.addItem(&self.edit_markdown_item(view, range));
                menu.addItem(&separator());
                self.add(Command::CopySection, &menu, None);
                menu.addItem(&self.rich_text_section_item(*index));
                self.add(Command::CopySectionLink, &menu, None);
                menu.addItem(&separator());
                self.add(Command::PromoteHeading, &menu, None);
                self.add(Command::DemoteHeading, &menu, None);
                menu.addItem(&separator());
                self.add(Command::FoldSection, &menu, None);
                self.add(Command::FoldAll, &menu, None);
                self.add(Command::MoveBlockUp, &menu, None);
                self.add(Command::MoveBlockDown, &menu, None);
            }

            ContextTargetKind::CodeBlock(range) => {
                let range = *range;
                menu.addItem(&self.edit_markdown_item(view, range));
                menu.addItem(&separator());
                let this = weak();
                menu.addItem(&action_item("Copy Code", mtm, move || {
                    if let Some(this) = this.load() {
                        this.copy_range(range, CopyFlavour::Plain);
                    }
                }));
                let this = weak();
                menu.addItem(&action_item("Save as File…", mtm, move || {
                    if let Some(this) = this.load() {
                        this.save_code_block(range);
                    }
                }));
                let this = weak();
                menu.addItem(&action_item("Open in Editor", mtm, move || {
                    if let Some(this) = this.load() {
                        this.open_code_block_in_editor(range);
                    }
                }));
            }

            ContextTargetKind::PathToken(token) => {
                menu.addItem(&self.edit_markdown_item(view, target.source_range));
                menu.addItem(&separator());
                // Missing paths stay inert on every surface, including this
                // one: no editor launch and no Finder reveal may run for a
                // target that does not exist (§ Workspace and path
                // resolution).
                let this = weak();
                let opened = token.clone();
                menu.addItem(&action_item("Open in Editor", mtm, move || {
                    if let Some(this) = this.load() {
                        let _ = this.open_path_token_in_editor(&opened);
                    }
                }));
                let this = weak();
                let revealed = token.clone();
                menu.addItem(&action_item("Reveal in Finder", mtm, move || {
                    if let Some(this) = this.load() {
                        let _ = this.reveal_path_token_in_finder(&revealed);
                    }
                }));
                let raw_path = token.raw_path.clone();
                menu.addItem(&action_item("Copy Path", mtm, move || copy_string_to_pasteboard(&raw_path)));
            }

            ContextTargetKind::Image(source) => {
                menu.addItem(&self.edit_markdown_item(view, target.source_range));
                menu.addItem(&separator());
                let this = weak();
                let lightbox_source = source.clone();
                menu.addItem(&action_item("Open in Lightbox", mtm, move || {
                    if let Some(this) = this.load() {
                        this.present_lightbox(&lightbox_source, None);
                    }
                }));
                let this = weak();
                let copied_source = source.clone();
                menu.addItem(&action_item("Save a Copy…", mtm, move || {
                    if let Some(this) = this.load() {
                        this.save_image_copy(&copied_source);
                    }
                }));
                let this = weak();
                let revealed_source = source.clone();
                menu.addItem(&action_item("Reveal in Finder", mtm, move || {
                    let Some(this) = this.load() else { return };
                    let Some(base) = this.markdown_document().url().map(|url| url.deleting_last_path_component())
                    else {
                        return;
                    };
                    let url = base.appending_path_component(&revealed_source).standardized_file_url();
                    let target = url.clone();
                    this.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, move || {
                        workspace_reveal(&target);
                    });
                }));
            }

            ContextTargetKind::Link(destination) => {
                menu.addItem(&self.edit_markdown_item(view, target.source_range));
                menu.addItem(&separator());
                let this = weak();
                let link_view = view.retain();
                let link_destination = destination.clone();
                let source_range = target.source_range;
                menu.addItem(&action_item("Open Link", mtm, move || {
                    let Some(this) = this.load() else { return };
                    this.markdown_text_view_did_activate_link(
                        &link_view,
                        &link_destination,
                        source_range,
                        NSEventModifierFlags::empty(),
                    );
                }));
                let copied = destination.clone();
                menu.addItem(&action_item("Copy Target", mtm, move || copy_string_to_pasteboard(&copied)));
            }

            ContextTargetKind::Table(range) => {
                let range = *range;
                let hit_offset = target.hit_offset;
                menu.addItem(&self.edit_markdown_item(view, range));
                menu.addItem(&separator());
                let this = weak();
                menu.addItem(&action_item("Insert Row", mtm, move || {
                    if let Some(this) = this.load() {
                        this.table_insert_row(range, hit_offset);
                    }
                }));
                let this = weak();
                menu.addItem(&action_item("Delete Row", mtm, move || {
                    if let Some(this) = this.load() {
                        this.table_delete_row(range, hit_offset);
                    }
                }));
                menu.addItem(&separator());
                for alignment in [TableAlignment::Left, TableAlignment::Center, TableAlignment::Right] {
                    let this = weak();
                    let title = format!("Align Column {}", foundation::capitalized(alignment.raw_value()));
                    menu.addItem(&action_item(&title, mtm, move || {
                        if let Some(this) = this.load() {
                            this.table_set_alignment(range, alignment, hit_offset);
                        }
                    }));
                }
                let this = weak();
                menu.addItem(&action_item("Realign Source", mtm, move || {
                    let Some(this) = this.load() else { return };
                    this.markdown_document().ensure_parsed_current();
                    let edits = Restructure::realign_table(&this.markdown_document().parsed(), range);
                    this.markdown_document().apply(&edits, "Realign Table", None);
                }));
            }

            ContextTargetKind::Selection => {
                // SAFETY: `copy:` is `NSText`'s action, answered by the view.
                let copy = crate::app::document_window_controller_actions::plain_menu_item(
                    "Copy",
                    Some(sel!(copy:)),
                    mtm,
                );
                let view_target: &AnyObject = view;
                unsafe { copy.setTarget(Some(view_target)) };
                menu.addItem(&copy);
                menu.addItem(&self.edit_markdown_item(view, target.source_range));
                menu.addItem(&separator());
                self.add(Command::CopyAsMarkdown, &menu, None);
                self.add(Command::CopyAsRichText, &menu, None);
                self.add(Command::CopyAsPlainText, &menu, None);
                menu.addItem(&separator());
                let this = weak();
                menu.addItem(&action_item("Add Comment…", mtm, move || {
                    if let Some(this) = this.load() {
                        this.present_add_review(ReviewKind::Comment);
                    }
                }));
                let this = weak();
                menu.addItem(&action_item("Suggest Replacement…", mtm, move || {
                    if let Some(this) = this.load() {
                        this.present_add_review(ReviewKind::Suggestion);
                    }
                }));
                self.add(Command::SpeakDocument, &menu, None);
                menu.addItem(&separator());
                self.add(Command::ConvertToBulletList, &menu, None);
                self.add(Command::ConvertToNumberedList, &menu, None);
                self.add(Command::ConvertToTaskList, &menu, None);
                self.add(Command::ConvertToBlockquote, &menu, None);
            }

            ContextTargetKind::Plain => {
                let block_range = self
                    .markdown_document()
                    .parsed()
                    .root
                    .block_at(target.source_range.location)
                    .map_or(target.source_range, |block| block.range);
                menu.addItem(&self.edit_markdown_item(view, block_range));
                menu.addItem(&separator());
                self.add(Command::TidyDocument, &menu, None);
            }
        }
        if menu.numberOfItems() == 0 { None } else { Some(menu) }
    }

    fn add(&self, command: Command, menu: &NSMenu, title: Option<&str>) {
        let item = MainMenu::command_item(command, self.delegates_mtm());
        if let Some(title) = title {
            item.setTitle(&NSString::from_str(title));
        }
        // SAFETY: the controller answers `performDownrightCommand:`.
        unsafe { item.setTarget(Some(self.as_target())) };
        menu.addItem(&item);
    }

    fn edit_markdown_item(&self, view: &MarkdownTextView, range: NSRange) -> Retained<NSMenuItem> {
        let view: ObjcWeak<MarkdownTextView> = ObjcWeak::new(view);
        action_item("Edit Markdown", self.delegates_mtm(), move || {
            if let Some(view) = view.load() {
                view.focus_source(range);
            }
            if let Some(view) = view.load()
                && let Some(window) = view.window()
            {
                window.makeFirstResponder(Some(&view));
            }
        })
    }

    fn rich_text_section_item(&self, index: usize) -> Retained<NSMenuItem> {
        let this: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
        action_item("Copy Section as Rich Text", self.delegates_mtm(), move || {
            let Some(this) = this.load() else { return };
            let parsed = this.markdown_document().parsed();
            if !(index < parsed.headings.len()) {
                return;
            }
            this.copy_range(parsed.headings[index].section_range, CopyFlavour::RichText);
        })
    }
}

/// `actionItem(_:handler:)`: a menu item whose target owns a closure.
fn action_item(title: &str, mtm: MainThreadMarker, handler: impl Fn() + 'static) -> Retained<NSMenuItem> {
    let item = crate::app::document_window_controller_actions::plain_menu_item(
        title,
        Some(sel!(run:)),
        mtm,
    );
    let target = BlockActionTarget::new(Box::new(handler), mtm);
    // SAFETY: `BlockActionTarget` answers `run:`; the represented object
    // keeps the target alive with the item.
    unsafe {
        item.setTarget(Some(&target));
        item.setRepresentedObject(Some(&target));
    }
    item
}

// MARK: - BlockActionTarget

pub struct BlockActionTargetIvars {
    handler: RefCell<Box<dyn Fn()>>,
}

define_class!(
    /// Menu items hold their target weakly, so a closure-backed item needs
    /// something to own the closure for as long as the menu exists.
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlockActionTarget"]
    #[ivars = BlockActionTargetIvars]
    pub struct BlockActionTarget;

    unsafe impl NSObjectProtocol for BlockActionTarget {}

    impl BlockActionTarget {
        #[unsafe(method(run:))]
        fn __run(&self, _sender: Option<&AnyObject>) {
            self.run();
        }
    }
);

impl BlockActionTarget {
    /// `init(handler:)`.
    pub fn new(handler: Box<dyn Fn()>, mtm: MainThreadMarker) -> Retained<BlockActionTarget> {
        let this = Self::alloc(mtm).set_ivars(BlockActionTargetIvars { handler: RefCell::new(handler) });
        unsafe { msg_send![super(this), init] }
    }

    /// `@objc run(_:)`.
    pub fn run(&self) {
        (self.ivars().handler.borrow())();
    }
}

// MARK: - Panels

impl TaskPanelDelegate for DocumentWindowControllerDelegates {
    fn task_panel_did_toggle_task_at(&self, panel: &TaskPanelView, mark_offset: isize) {
        if let Some(controller) = self.controller() {
            controller.task_panel_did_toggle_task_at(panel, mark_offset);
        }
    }

    fn task_panel_did_select_task_at(&self, panel: &TaskPanelView, content_offset: isize) {
        if let Some(controller) = self.controller() {
            controller.task_panel_did_select_task_at(panel, content_offset);
        }
    }

    fn task_panel_did_request_new_task(&self, panel: &TaskPanelView, text: &str, heading_index: Option<isize>) {
        if let Some(controller) = self.controller() {
            controller.task_panel_did_request_new_task(panel, text, heading_index);
        }
    }

    fn task_panel_did_move_task(&self, panel: &TaskPanelView, task_index: isize, before: Option<isize>) {
        if let Some(controller) = self.controller() {
            controller.task_panel_did_move_task(panel, task_index, before);
        }
    }
}

impl DocumentWindowController {
    /// `taskPanel(_:didToggleTaskAt:)`.
    pub fn task_panel_did_toggle_task_at(&self, _panel: &TaskPanelView, mark_offset: isize) {
        self.markdown_document().ensure_parsed_current();
        self.primary_container().text_view().preserve_viewport_on_next_document_update();
        if let Some(split) = self.split_container() {
            split.text_view().preserve_viewport_on_next_document_update();
        }
        self.markdown_document().toggle_task(mark_offset);
        self.refit_floating_surface_after_content_change();
    }

    /// `taskPanel(_:didSelectTaskAt:)`.
    pub fn task_panel_did_select_task_at(&self, _panel: &TaskPanelView, content_offset: isize) {
        self.jump(content_offset, "Task", true);
    }

    /// `taskPanel(_:didRequestNewTask:headingIndex:)`.
    pub fn task_panel_did_request_new_task(&self, _panel: &TaskPanelView, text: &str, heading_index: Option<isize>) {
        self.markdown_document().ensure_parsed_current();
        let edits = Restructure::insert_task(&self.markdown_document().parsed(), text, heading_index);
        self.commit_task_edits(&edits, "Add Task");
    }

    /// `taskPanel(_:didMoveTask:before:)`.
    pub fn task_panel_did_move_task(&self, _panel: &TaskPanelView, task_index: isize, target_index: Option<isize>) {
        self.markdown_document().ensure_parsed_current();
        let edits = Restructure::move_task(&self.markdown_document().parsed(), task_index, target_index);
        self.commit_task_edits(&edits, "Move Task");
    }

    /// Task edits behave like checkbox toggles (§7.1): the document writes
    /// through immediately, the viewport does not jump, and the parse — which
    /// repopulates the panel — lands in the same turn.
    fn commit_task_edits(&self, edits: &[TextEdit], action_name: &str) {
        if edits.is_empty() {
            return;
        }
        self.primary_container().text_view().preserve_viewport_on_next_document_update();
        if let Some(split) = self.split_container() {
            split.text_view().preserve_viewport_on_next_document_update();
        }
        self.markdown_document().apply(edits, action_name, None);
        self.markdown_document().reparse_now(false);
        if self.markdown_document().url().is_some() {
            let _ = self.markdown_document().save_if_needed(SaveIntent::Normal);
        }
        self.refit_floating_surface_after_content_change();
    }
}

impl DensityGutterDelegate for DocumentWindowControllerDelegates {
    fn density_gutter_did_request_scroll_to_fraction(&self, gutter: &DensityGutterView, fraction: CGFloat) {
        if let Some(controller) = self.controller() {
            controller.density_gutter_did_request_scroll_to_fraction(gutter, fraction);
        }
    }

    fn density_gutter_preview_at_fraction(
        &self,
        gutter: &DensityGutterView,
        fraction: CGFloat,
    ) -> Option<(String, String, String)> {
        self.controller()?.density_gutter_preview_at_fraction(gutter, fraction)
    }
}

impl DocumentWindowController {
    /// `densityGutter(_:didRequestScrollToFraction:)`.
    pub fn density_gutter_did_request_scroll_to_fraction(&self, gutter: &DensityGutterView, fraction: CGFloat) {
        let offset = (fraction * self.markdown_document().parsed().length as CGFloat) as isize;
        self.container_text_view().scroll_to_offset(offset, ScrollPosition::Top, !gutter.is_scrubbing());
        self.update_breadcrumb_and_gutter();
    }

    /// `densityGutter(_:previewAtFraction:)`.
    pub fn density_gutter_preview_at_fraction(
        &self,
        _gutter: &DensityGutterView,
        fraction: CGFloat,
    ) -> Option<(String, String, String)> {
        let parsed = self.markdown_document().parsed();
        let offset = (fraction * parsed.length as CGFloat) as isize;
        let Some(index) = parsed.headings.iter().rposition(|heading| heading.range.location <= offset) else {
            return Some(("Document start".to_owned(), String::new(), self.density_gutter_view().metrics_summary()));
        };
        let heading = &parsed.headings[index];
        let section_position = format!("Section {} of {}", index + 1, parsed.headings.len());
        let context = if heading.word_count > 0 {
            format!("{section_position} · {} words", heading.word_count)
        } else {
            section_position
        };
        let container_text_view = self.container_text_view();
        if container_text_view.mode() == RenderMode::Source || container_text_view.source_focus() == SourceFocus::Document {
            let snippet_length = 160.min(0.max(parsed.length - offset));
            return Some((heading.title.clone(), parsed.substring(NSRange::new(offset, snippet_length)), context));
        }
        Some((
            heading.title.clone(),
            StructuralZoom::section_preview(&parsed, index as isize).unwrap_or_else(|| "Section overview".to_owned()),
            context,
        ))
    }
}

impl BreadcrumbDelegate for DocumentWindowControllerDelegates {
    fn breadcrumb_did_select_heading_at(&self, view: &BreadcrumbView, index: isize) {
        if let Some(controller) = self.controller() {
            controller.breadcrumb_did_select_heading_at(view, index);
        }
    }
}

impl DocumentWindowController {
    /// `breadcrumb(_:didSelectHeadingAt:)`.
    pub fn breadcrumb_did_select_heading_at(&self, _view: &BreadcrumbView, index: isize) {
        let parsed = self.markdown_document().parsed();
        if !(index < parsed.headings.len() as isize) {
            return;
        }
        let heading = &parsed.headings[index as usize];
        self.jump(heading.range.location, &heading.title, true);
    }
}

impl FindBarDelegate for DocumentWindowControllerDelegates {
    fn find_bar_did_change(&self, bar: &FindBarView, query: FindQuery) {
        if let Some(controller) = self.controller() {
            controller.find_bar_did_change(bar, query);
        }
    }

    fn find_bar_did_request_advance(&self, bar: &FindBarView, forward: bool) {
        if let Some(controller) = self.controller() {
            controller.find_bar_did_request_advance(bar, forward);
        }
    }

    fn find_bar_did_request_replace(&self, bar: &FindBarView, replacement: &str, all: bool) {
        if let Some(controller) = self.controller() {
            controller.find_bar_did_request_replace(bar, replacement, all);
        }
    }

    fn find_bar_did_request_close(&self, bar: &FindBarView) {
        if let Some(controller) = self.controller() {
            controller.find_bar_did_request_close(bar);
        }
    }
}

impl DocumentWindowController {
    /// `findBar(_:didChange:)`.
    pub fn find_bar_did_change(&self, _bar: &FindBarView, query: FindQuery) {
        self.schedule_find_query(query);
    }

    /// `findBar(_:didRequestAdvance:)`.
    pub fn find_bar_did_request_advance(&self, bar: &FindBarView, forward: bool) {
        self.flush_find_query_if_needed(bar.current_query());
        self.advance_find(forward);
    }

    /// `findBar(_:didRequestReplace:all:)`.
    pub fn find_bar_did_request_replace(&self, bar: &FindBarView, replacement: &str, all: bool) {
        self.flush_find_query_if_needed(bar.current_query());
        if all {
            let edits =
                FindEngine::replace_all_edits(&self.markdown_document().text(), &self.current_find_query(), replacement);
            self.markdown_document().apply(&edits, "Replace All", None);
            self.run_find(self.current_find_query(), true, true);
            self.replace_results(&format!("Replaced {}", edits.len()));
            return;
        }
        let text = self.markdown_document().text();
        let caret = self.container_text_view().source_selected_range().location;
        let edit = self.find_session().borrow_mut().replacement_edit(&text, replacement, caret);
        if let Some(edit) = edit {
            self.apply_in_place_document_edits(&[edit], "Replace", None);
            self.run_find(self.current_find_query(), true, true);
            self.replace_results("Replaced 1");
        } else {
            // Nothing to replace (empty bar). Surface that rather than
            // silently doing nothing when the pill is in replace mode.
            self.replace_results("Nothing to replace");
        }
    }

    /// `findBarDidRequestClose(_:)`.
    pub fn find_bar_did_request_close(&self, _bar: &FindBarView) {
        self.dismiss_find_bar();
    }

    /// A button or Return must act on the text the reader can see, even when
    /// it lands inside the find-as-you-type debounce window.
    fn flush_find_query_if_needed(&self, query: FindQuery) {
        if query == self.current_find_query() {
            return;
        }
        self.run_find(query, false, true);
    }

    /// A short-lived row confirmation after a replace, so the reader sees the
    /// result instead of a stale "N of M" count.
    fn replace_results(&self, message: &str) {
        if let Some(bar) = self.find_bar() {
            bar.set_status_text(message);
        }
    }
}

impl ConflictBarDelegate for DocumentWindowControllerDelegates {
    fn conflict_bar_did_request_review(&self, bar: &ConflictBarView) {
        if let Some(controller) = self.controller() {
            controller.conflict_bar_did_request_review(bar);
        }
    }

    fn conflict_bar_did_request_keep_mine(&self, bar: &ConflictBarView) {
        if let Some(controller) = self.controller() {
            controller.conflict_bar_did_request_keep_mine(bar);
        }
    }

    fn conflict_bar_did_request_take_theirs(&self, bar: &ConflictBarView) {
        if let Some(controller) = self.controller() {
            controller.conflict_bar_did_request_take_theirs(bar);
        }
    }

    fn conflict_bar_did_request_dismiss(&self, bar: &ConflictBarView) {
        if let Some(controller) = self.controller() {
            controller.conflict_bar_did_request_dismiss(bar);
        }
    }
}

impl DocumentWindowController {
    /// `conflictBarDidRequestReview(_:)`.
    pub fn conflict_bar_did_request_review(&self, _bar: &ConflictBarView) {
        let Some(conflict) = self.pending_conflict() else { return };
        let Some(url) = self.markdown_document().url() else { return };
        let controller = CompareWindowController::new(
            &self.markdown_document().text(),
            "Yours",
            &conflict.incoming_text,
            "On disk",
            Some(&url),
            self.delegates_mtm(),
        );
        controller.showWindow(None);
        self.retain_timeline(&controller);
    }

    /// `conflictBarDidRequestKeepMine(_:)`.
    pub fn conflict_bar_did_request_keep_mine(&self, _bar: &ConflictBarView) {
        if self.markdown_document().resolve_conflict_keeping_mine().is_ok() {
            self.dismiss_conflict_bar();
        }
    }

    /// `conflictBarDidRequestTakeTheirs(_:)`.
    pub fn conflict_bar_did_request_take_theirs(&self, _bar: &ConflictBarView) {
        let Some(conflict) = self.pending_conflict() else { return };
        self.markdown_document().resolve_conflict_taking_theirs(&conflict);
        self.dismiss_conflict_bar();
    }

    /// `conflictBarDidRequestDismiss(_:)`.
    pub fn conflict_bar_did_request_dismiss(&self, _bar: &ConflictBarView) {
        self.dismiss_conflict_bar();
    }
}

impl ChangeSummaryBarDelegate for DocumentWindowControllerDelegates {
    fn change_summary_bar_did_request_jump(&self, bar: &ChangeSummaryBarView, forward: bool) {
        if let Some(controller) = self.controller() {
            controller.change_summary_bar_did_request_jump(bar, forward);
        }
    }

    fn change_summary_bar_did_request_mark_reviewed(&self, bar: &ChangeSummaryBarView) {
        if let Some(controller) = self.controller() {
            controller.change_summary_bar_did_request_mark_reviewed(bar);
        }
    }

    fn change_summary_bar_did_request_dismiss(&self, bar: &ChangeSummaryBarView) {
        if let Some(controller) = self.controller() {
            controller.change_summary_bar_did_request_dismiss(bar);
        }
    }
}

impl DocumentWindowController {
    /// `changeSummaryBar(_:didRequestJump:)`.
    pub fn change_summary_bar_did_request_jump(&self, _bar: &ChangeSummaryBarView, forward: bool) {
        self.perform(if forward { Command::NextChange } else { Command::PreviousChange });
    }

    /// `changeSummaryBarDidRequestMarkReviewed(_:)`.
    pub fn change_summary_bar_did_request_mark_reviewed(&self, _bar: &ChangeSummaryBarView) {
        self.markdown_document().changes().clear();
        self.dismiss_change_summary();
    }

    /// `changeSummaryBarDidRequestDismiss(_:)`.
    pub fn change_summary_bar_did_request_dismiss(&self, _bar: &ChangeSummaryBarView) {
        self.dismiss_change_summary();
    }
}

impl TidySheetDelegate for DocumentWindowControllerDelegates {
    fn tidy_sheet_did_apply(&self, sheet: &TidySheetView, edits: &[TextEdit]) {
        if let Some(controller) = self.controller() {
            controller.tidy_sheet_did_apply(sheet, edits);
        }
    }

    fn tidy_sheet_did_cancel(&self, sheet: &TidySheetView) {
        if let Some(controller) = self.controller() {
            controller.tidy_sheet_did_cancel(sheet);
        }
    }
}

impl DocumentWindowController {
    /// `tidySheet(_:didApply:)`.
    pub fn tidy_sheet_did_apply(&self, _sheet: &TidySheetView, edits: &[TextEdit]) {
        self.markdown_document().apply(edits, "Tidy Document", None);
        self.close_tidy_sheet();
    }

    /// `tidySheetDidCancel(_:)`.
    pub fn tidy_sheet_did_cancel(&self, _sheet: &TidySheetView) {
        self.close_tidy_sheet();
    }

    fn close_tidy_sheet(&self) {
        let Some(sheet_window) = self.tidy_sheet_window() else { return };
        if let Some(window) = self.window() {
            window.endSheet(&sheet_window);
        }
        self.set_tidy_sheet_window(None);
    }
}

impl SearchResultsDelegate for DocumentWindowControllerDelegates {
    fn search_results_did_select(&self, view: &SearchResultsPanelView, hit: &SiblingHit) {
        if let Some(controller) = self.controller() {
            controller.search_results_did_select(view, hit);
        }
    }
}

impl DocumentWindowController {
    /// `searchResults(_:didSelect:)`.
    pub fn search_results_did_select(&self, _view: &SearchResultsPanelView, hit: &SiblingHit) {
        if self.markdown_document().url().is_some_and(|url| swift_text::str_eq(&hit.url.path(), &url.path())) {
            self.jump(hit.range.location, "Search hit", true);
        } else if let Some(controller) = app_delegate(self.delegates_mtm())
            .and_then(|delegate| delegate.open(&hit.url, None, None, false, DocumentOpenDisposition::Tab, None))
        {
            controller.jump(hit.range.location, "Search hit", true);
        }
    }
}

impl HistoryInspectorViewDelegate for DocumentWindowControllerDelegates {
    fn history_inspector_did_request_full_history(&self, inspector: &HistoryInspectorView) {
        if let Some(controller) = self.controller() {
            controller.history_inspector_did_request_full_history(inspector);
        }
    }

    fn history_inspector_did_request_restore(&self, inspector: &HistoryInspectorView, record: &VersionRecord) {
        if let Some(controller) = self.controller() {
            controller.history_inspector_did_request_restore(inspector, record);
        }
    }
}

impl DocumentWindowController {
    /// `historyInspectorDidRequestFullHistory(_:)`.
    pub fn history_inspector_did_request_full_history(&self, _inspector: &HistoryInspectorView) {
        self.perform(Command::VersionTimeline);
    }

    /// `historyInspector(_:didRequestRestore:)`.
    pub fn history_inspector_did_request_restore(&self, inspector: &HistoryInspectorView, record: &VersionRecord) {
        let alert = NSAlert::new(self.delegates_mtm());
        alert.setMessageText(&NSString::from_str("Restore this version?"));
        alert.setInformativeText(&NSString::from_str(
            "The current text will be replaced with the selected version. This is an ordinary edit, so ⌘Z undoes it.",
        ));
        alert.addButtonWithTitle(&NSString::from_str("Restore"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        if alert.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        self.markdown_document().restore(record);
        inspector.set_versions(self.markdown_document().versions());
    }
}
