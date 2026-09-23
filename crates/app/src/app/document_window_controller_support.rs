//! Port of `App/DocumentWindowController+Support.swift`: accessors,
//! copy/export, the sheets the command switch reaches for, sibling search,
//! [`PlainTextRenderer`] and [`PrintRenderer`].
//!
//! `NativeFragmentImageProvider` (the file's third type) lives in
//! `export::html_exporter`, where the HTML exporter uses it.

use std::rc::{Rc, Weak};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
    NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey, NSAccessibilityPriorityLevel,
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSAlertStyle, NSAlertThirdButtonReturn,
    NSApplication, NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep, NSModalResponse, NSModalResponseOK,
    NSPasteboard, NSPasteboardTypeRTF, NSPasteboardTypeString, NSPrintInfo, NSPrintJobSavingURL, NSPrintOperation,
    NSPrintSaveJob, NSPrintingPaginationMode, NSSavePanel, NSTextView, NSWindow, NSWindowStyleMask,
    NSAttributedStringDocumentReadingOptionKey, NSCharacterEncodingDocumentOption, NSDocumentTypeDocumentOption,
    NSHTMLTextDocumentType, NSAttributedStringAppKitDocumentFormats, NSAttributedStringDocumentFormats,
};
use objc2_foundation::{
    NSArray, NSCocoaErrorDomain, NSData, NSDataWritingOptions, NSDictionary, NSError, NSLocalizedDescriptionKey, NSMutableDictionary, NSNumber, NSRange as FoundationRange, NSSize, NSString,
    NSStringCompareOptions, NSUTF8StringEncoding,
};
use objc2_uniform_type_identifiers::{UTType, UTTypeHTML, UTTypePDF, UTTypePNG};
use upleft_core::document_io::DocumentIO;
use upleft_core::ns_range::ns_intersection_range;
use upleft_core::{BlockContent, NSRange, ParsedDocument, TextEdit};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::rect;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::ai::markdown_document::{DocumentError, SaveError, SaveIntent};
use crate::app::app_delegate::{AppDelegate, DocumentOpenDisposition};
use crate::app::document_types;
use crate::app::document_window_controller::{DocumentWindowController, SiblingSearchCancellationToken};
use crate::export::html_exporter::{HTMLExporter, NativeFragmentImageProvider};
use crate::panels::appkit_support::downcast;
use crate::panels::inspector_host_view::InspectorSection;
use crate::panels::panel_chrome::panel_title;
use crate::panels::search_results_panel_view::{SearchResultsDelegate, SearchResultsPanelView};
use crate::panels::tidy_sheet_view::{TidySheetDelegate, TidySheetView};
use crate::support::commands::Command;
use crate::support::find_engine::{FindEngine, FindQuery};

/// `DocumentWindowController.CopyFlavour`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFlavour {
    Markdown,
    RichText,
    Plain,
}

impl DocumentWindowController {
    // MARK: - Accessors

    /// `var containerTextView`: the split pane when it is first responder,
    /// else the primary pane.
    pub fn container_text_view(&self) -> Retained<MarkdownTextView> {
        if let Some(split) = self.split_container().map(|container| container.text_view().clone())
            && self.window().and_then(|window| window.firstResponder()).is_some_and(|responder| {
                std::ptr::eq(&*responder as *const _ as *const AnyObject, &*split as *const _ as *const AnyObject)
            })
        {
            return split;
        }
        self.primary_container().text_view().clone()
    }

    /// `var currentStyleSheet`.
    pub fn current_style_sheet(&self) -> Rc<StyleSheet> {
        self.active_style_sheet()
    }

    /// `caretOffset()`.
    pub fn caret_offset(&self) -> isize {
        let selection = self.container_text_view().source_selected_range();
        if selection.length > 0 { selection.location } else { 0.max(selection.location) }
    }

    /// With no selection, commands act on the caret's block, which is what
    /// makes ⌘B and the convert commands usable without selecting first.
    pub fn selection_range(&self) -> NSRange {
        let selection = self.container_text_view().source_selected_range();
        if selection.length != 0 {
            return selection;
        }
        let parsed = self.markdown_document().parsed();
        match parsed.root.block_at(selection.location) {
            Some(block) => block.content_range,
            None => selection,
        }
    }

    /// `currentHeadingIndex()`.
    pub fn current_heading_index(&self) -> Option<usize> {
        let offset = self.caret_offset();
        self.markdown_document().parsed().headings.iter().rposition(|heading| heading.range.location <= offset)
    }

    // MARK: - Copy flavours (§9.5)

    /// `copy(flavour:)`.
    pub fn copy(&self, flavour: CopyFlavour) {
        let range = self.container_text_view().source_selected_range();
        let effective = if range.length > 0 {
            range
        } else {
            NSRange { location: 0, length: self.markdown_document().parsed().length }
        };
        self.copy_range(effective, flavour);
    }

    /// `copy(range:flavour:)`.
    pub fn copy_range(&self, range: NSRange, flavour: CopyFlavour) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let text = self.markdown_document().storage().string();
        let markdown = text.substringWithRange(FoundationRange::new(range.location as usize, range.length as usize));

        match flavour {
            CopyFlavour::Markdown => {
                pasteboard.setString_forType(&markdown, unsafe { NSPasteboardTypeString });
            }
            CopyFlavour::RichText => {
                let attributed = self.container_text_view().exportable_attributed_string(range);
                let rtf = unsafe {
                    attributed.RTFFromRange_documentAttributes(
                        FoundationRange::new(0, attributed.length()),
                        &NSDictionary::new(),
                    )
                };
                if let Some(rtf) = rtf {
                    let types = NSArray::from_slice(&[unsafe { NSPasteboardTypeRTF }, unsafe { NSPasteboardTypeString }]);
                    unsafe { pasteboard.declareTypes_owner(&types, None) };
                    pasteboard.setData_forType(Some(&rtf), unsafe { NSPasteboardTypeRTF });
                    pasteboard.setString_forType(&attributed.string(), unsafe { NSPasteboardTypeString });
                } else {
                    pasteboard.setString_forType(&attributed.string(), unsafe { NSPasteboardTypeString });
                }
            }
            CopyFlavour::Plain => {
                let plain = PlainTextRenderer::render(&self.markdown_document().parsed(), range);
                pasteboard.setString_forType(&NSString::from_str(&plain), unsafe { NSPasteboardTypeString });
            }
        }
    }

    /// `copyCurrentSection()`.
    pub fn copy_current_section(&self) {
        let Some(index) = self.current_heading_index() else { return };
        let section = self.markdown_document().parsed().headings[index].section_range;
        self.copy_range(section, CopyFlavour::Markdown);
    }

    /// Copies a link to a section. `index` names the heading the user
    /// pointed at; without one the caret's section is the sensible default.
    pub fn copy_section_link(&self, index: Option<usize>) {
        let Some(index) = index.or_else(|| self.current_heading_index()) else { return };
        let parsed = self.markdown_document().parsed();
        if index >= parsed.headings.len() {
            return;
        }
        let Some(url) = self.markdown_document().url() else { return };
        let heading = &parsed.headings[index];
        let link = format!("[{}]({}#{})", heading.title, url.last_path_component(), heading.slug);
        NSPasteboard::generalPasteboard().clearContents();
        NSPasteboard::generalPasteboard()
            .setString_forType(&NSString::from_str(&link), unsafe { NSPasteboardTypeString });
        self.announce_transient_status(&format!("Copied link to \u{201C}{}\u{201D}", heading.title));
    }

    /// A quiet, non-blocking confirmation for actions whose only other
    /// evidence is a changed clipboard.
    pub fn announce_transient_status(&self, message: &str) {
        let Some(element) = self.window() else { return };
        let value = NSString::from_str(message);
        let priority = NSNumber::new_isize(NSAccessibilityPriorityLevel::Medium.0);
        let user_info = unsafe {
            NSDictionary::from_slices(
                &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
                &[value.as_ref() as &AnyObject, priority.as_ref() as &AnyObject],
            )
        };
        unsafe {
            NSAccessibilityPostNotificationWithUserInfo(
                &element,
                NSAccessibilityAnnouncementRequestedNotification,
                Some(&user_info),
            )
        };
    }

    // MARK: - Saving

    /// `saveDocument()`.
    pub fn save_document(&self) -> bool {
        self.markdown_document().save(SaveIntent::Normal).is_ok()
    }

    /// `presentSaveError(_:)`.
    pub fn present_save_error(&self, error: &DocumentError) {
        match error.save_error() {
            Some(SaveError::FileMissing(_)) | Some(SaveError::FileUnreadable(..)) => self.present_save_recovery(error),
            _ => self.present_operation_error(
                &format!("Couldn\u{2019}t save {}", self.markdown_document().display_name()),
                &error.localized_description(),
            ),
        }
    }

    fn present_save_recovery(&self, error: &DocumentError) {
        if self.save_recovery_alert().is_some() {
            return;
        }
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str("The original file can\u{2019}t be saved safely"));
        alert.setInformativeText(&NSString::from_str(&error.localized_description()));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.addButtonWithTitle(&NSString::from_str("Save a Copy\u{2026}"));
        alert.addButtonWithTitle(&NSString::from_str("Recreate File"));
        alert.addButtonWithTitle(&NSString::from_str("Discard Changes"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        let buttons = alert.buttons();
        buttons.objectAtIndex(0).setToolTip(Some(&NSString::from_str("Write your Markdown to a different path")));
        buttons
            .objectAtIndex(1)
            .setToolTip(Some(&NSString::from_str("Explicitly create or replace the original path")));
        buttons.objectAtIndex(2).setToolTip(Some(&NSString::from_str("Stop trying to save these local edits")));
        buttons.objectAtIndex(3).setToolTip(Some(&NSString::from_str("Keep editing without writing anything")));
        self.set_save_recovery_alert(Some(alert.clone()));

        let weak = objc2::rc::Weak::new(self);
        let handle = move |response: NSModalResponse| {
            let Some(this) = weak.load() else { return };
            this.set_save_recovery_alert(None);
            if response == NSAlertFirstButtonReturn {
                this.save_as();
            } else if response == NSAlertSecondButtonReturn {
                let _ = this.markdown_document().recreate_missing_file();
            } else if response == NSAlertThirdButtonReturn {
                this.markdown_document().discard_unsaved_changes();
            }
        };
        if let Some(window) = self.window() {
            let handler = block2::RcBlock::new(handle);
            alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
        } else {
            handle(alert.runModal());
        }
    }

    /// `presentOperationError(_:error:)`: `error_description` is the Swift
    /// error's `localizedDescription`.
    pub fn present_operation_error(&self, title: &str, error_description: &str) {
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(error_description));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        if let Some(window) = self.window() {
            alert.beginSheetModalForWindow_completionHandler(&window, None);
        } else {
            alert.runModal();
        }
    }

    /// `saveAs()`.
    pub fn save_as(&self) {
        let mtm = self.mtm();
        let panel = NSSavePanel::savePanel(mtm);
        let types: Vec<Retained<UTType>> = document_types::content_types();
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
        let name = self
            .markdown_document()
            .url()
            .map(|url| url.last_path_component())
            .unwrap_or_else(|| "Untitled.md".to_owned());
        panel.setNameFieldStringValue(&NSString::from_str(&name));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(url) = panel.URL().and_then(|url| FileUrl::from_nsurl(&url)) else { return };
        // A "copy" must be byte-faithful to the source, not normalised to
        // .default: a CRLF / UTF-16 / BOM / no-final-newline document saved
        // as a copy should stay exactly that (§3.1).
        if let Err(error) = DocumentIO::write(
            &self.markdown_document().text(),
            std::path::Path::new(&url.path()),
            self.markdown_document().fidelity(),
        ) {
            self.present_operation_error("Couldn\u{2019}t save a copy", &error.to_string());
            return;
        }
        let app = NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate().and_then(|delegate| downcast::<AppDelegate>(delegate.as_ref())) {
            let _ = delegate.open(&url, Some(self.mode()), None, false, DocumentOpenDisposition::Tab, None);
        }
    }

    // MARK: - Export (§9.5)

    /// `exporter(forPrint:)`.
    pub fn exporter(&self, for_print: bool) -> HTMLExporter {
        // Export captures the current buffer even while its async parse is
        // pending.
        self.markdown_document().ensure_parsed_current();
        let style_sheet = self.current_style_sheet();
        let mut exporter = HTMLExporter::new(
            self.markdown_document().parsed(),
            style_sheet.theme.clone(),
            self.markdown_document().display_name(),
            self.markdown_document().url().map(|url| url.deleting_last_path_component()),
            Some(Box::new(NativeFragmentImageProvider::new((*style_sheet).clone()))),
        );
        exporter.for_print = for_print;
        exporter
    }

    /// `exportHTML()`.
    pub fn export_html(&self) {
        let panel = NSSavePanel::savePanel(self.mtm());
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[unsafe { UTTypeHTML }.retain()]));
        panel.setNameFieldStringValue(&NSString::from_str(&(self.markdown_document().display_name() + ".html")));
        panel.setMessage(Some(&NSString::from_str("Export a self-contained HTML file")));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(url) = panel.URL() else { return };
        self.begin_activity();
        let html = self.exporter(false).html();
        let data = NSData::with_bytes(html.as_bytes());
        if let Err(error) = data.writeToURL_options_error(&url, NSDataWritingOptions::empty()) {
            self.present_operation_error("Couldn\u{2019}t export HTML", &error.localizedDescription().to_string());
        }
        // `defer { endActivity() }`
        self.end_activity();
    }

    /// `exportPDF()`.
    pub fn export_pdf(&self) {
        let panel = NSSavePanel::savePanel(self.mtm());
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[unsafe { UTTypePDF }.retain()]));
        panel.setNameFieldStringValue(&NSString::from_str(&(self.markdown_document().display_name() + ".pdf")));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(url) = panel.URL() else { return };
        self.begin_activity();
        if !PrintRenderer::write_pdf(&self.exporter(true).html(), &url, self.mtm()) {
            let error = error_with_description("Upleft.Export", 1, "The PDF renderer could not write the selected file.");
            self.present_operation_error("Couldn\u{2019}t export PDF", &error.localizedDescription().to_string());
        }
        // `defer { endActivity() }`
        self.end_activity();
    }

    /// `printDocument()`: a stylesheet designed for paper, not a screenshot
    /// of the screen theme (§9.5).
    pub fn print_document(&self) {
        self.begin_activity();
        PrintRenderer::print(&self.exporter(true).html(), &self.markdown_document().display_name(), self.mtm());
        // `defer { endActivity() }`
        self.end_activity();
    }

    /// `exportSelectionAsImage()`.
    pub fn export_selection_as_image(&self) {
        let range = self.container_text_view().source_selected_range();
        if !(range.length > 0) {
            return;
        }
        let Some(image) = self.container_text_view().image_for_selection(range) else { return };
        let panel = NSSavePanel::savePanel(self.mtm());
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[unsafe { UTTypePNG }.retain()]));
        panel.setNameFieldStringValue(&NSString::from_str(
            &(self.markdown_document().display_name() + " selection.png"),
        ));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(url) = panel.URL() else { return };
        let Some(tiff) = image.TIFFRepresentation() else { return };
        let Some(bitmap) = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff) else { return };
        let Some(png) =
            (unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) })
        else {
            return;
        };
        if let Err(error) = png.writeToURL_options_error(&url, NSDataWritingOptions::empty()) {
            self.present_operation_error(
                "Couldn\u{2019}t export the selection",
                &error.localizedDescription().to_string(),
            );
        }
    }

    // MARK: - Sheets and panels

    /// `presentTidySheet(_:)`.
    pub fn present_tidy_sheet(&self, edits: &[TextEdit]) {
        let mtm = self.mtm();
        let Some(window) = self.window() else { return };
        let sheet_window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 680.0, 520.0),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // The sheet is owned by `tidySheetWindow`, as Swift's ARC owns it.
        unsafe { sheet_window.setReleasedWhenClosed(false) };
        sheet_window.setTitle(&NSString::from_str("Tidy Document"));
        sheet_window.setMinSize(NSSize::new(560.0, 400.0));

        let sheet = TidySheetView::new_current(mtm);
        sheet.set_style_sheet(self.current_style_sheet());
        let text = self.markdown_document().storage().string();
        let proposals = edits
            .iter()
            .map(|edit| {
                let before = text
                    .substringWithRange(FoundationRange::new(edit.range.location as usize, edit.range.length as usize))
                    .to_string();
                (edit.clone(), before, edit.replacement.clone())
            })
            .collect();
        sheet.set_proposals(proposals);
        let delegate: Weak<dyn TidySheetDelegate> = Rc::downgrade(&self.delegates()) as _;
        sheet.set_delegate(Some(delegate));
        sheet_window.setContentView(Some(&sheet));
        sheet.reload();

        self.set_tidy_sheet_window(Some(sheet_window.clone()));
        window.beginSheet_completionHandler(&sheet_window, None);
    }

    /// `showSiblingSearch()`.
    pub fn show_sibling_search(&self) {
        if self.scanner().is_none() {
            return;
        }
        self.show_find_inspector(false);
        // The header names the surface that opened (§7.2).
        if let Some(host) = self.inspector_host() {
            host.set_title(&panel_title(Command::FindInSiblings), InspectorSection::Search);
        }
        if self.search_results().is_none() {
            let panel = SearchResultsPanelView::new_current(self.mtm());
            let delegate: Weak<dyn SearchResultsDelegate> = Rc::downgrade(&self.delegates()) as _;
            panel.set_delegate(Some(delegate));
            panel.set_style_sheet(self.current_style_sheet());
            self.set_search_results(Some(panel));
        }
        // Ordinary Find replaces the SearchInspector but deliberately retains
        // the shared session. Reattach the retained results view whenever this
        // surface opens so background work never updates a detached panel.
        if let Some(inspector) = self.search_inspector() {
            let results = self.search_results();
            inspector.set_results(results.as_deref().map(|panel| panel as &objc2_app_kit::NSView));
        }

        // The field is the authority for what the reader can see.
        let query = self.find_bar().map(|bar| bar.current_query()).unwrap_or_default();
        self.run_find(query, false, true);
    }

    /// Immediately retires results for the old visible query.
    pub fn stage_sibling_search(&self, query: &FindQuery) {
        if !self.sibling_search_active() {
            return;
        }
        if let Some(token) = self.sibling_search_cancellation() {
            token.cancel();
        }
        self.set_sibling_search_cancellation(None);
        self.set_sibling_search_generation(self.sibling_search_generation().wrapping_add(1));
        if let Some(results) = self.search_results() {
            results.set_query(&query.text);
        }
        if let Some(results) = self.search_results() {
            results.set_searched_file_count(
                self.scanner().map(|scanner| scanner.siblings().len() as isize).unwrap_or(0),
            );
        }
        if let Some(results) = self.search_results() {
            results.set_hits(Vec::new());
        }
        if let Some(results) = self.search_results() {
            results.set_is_searching(!query.is_empty() && FindEngine::is_valid(query));
        }
    }

    /// Runs the on-demand sibling scan for the query currently visible in the
    /// field. Each pass owns a generation so a slow result can never replace
    /// a newer one.
    pub fn refresh_sibling_search(&self, query: &FindQuery) {
        if !self.sibling_search_active()
            || self.inspector_host().and_then(|host| host.selected_section()) != Some(InspectorSection::Search)
        {
            return;
        }
        let Some(scanner) = self.scanner() else { return };
        self.stage_sibling_search(query);
        let generation = self.sibling_search_generation();
        if query.is_empty() || !FindEngine::is_valid(query) {
            return;
        }

        let urls: Vec<FileUrl> = scanner.siblings().into_iter().map(|sibling| sibling.url).collect();
        let scanner_id = Rc::as_ptr(&scanner) as usize;
        let runner = self.sibling_search_runner();
        // A selection range belongs to the open document. Applying it to
        // every sibling would search arbitrary offsets in unrelated files.
        let mut normalized_query = query.clone();
        normalized_query.scope = None;
        let cross_file_query = normalized_query;
        let cancellation = Arc::new(SiblingSearchCancellationToken::new());
        self.set_sibling_search_cancellation(Some(cancellation.clone()));

        let handle = self.handle();
        let query = query.clone();
        self.sibling_search_queue().exec_async(move || {
            if cancellation.is_cancelled() {
                return;
            }
            let token = cancellation.clone();
            let hits = runner(&cross_file_query, &urls, &move || token.is_cancelled());
            if cancellation.is_cancelled() {
                return;
            }
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(this) = handle.load() else { return };
                if !(this.sibling_search_active()
                    && this.sibling_search_generation() == generation
                    && this.sibling_search_cancellation().is_some_and(|current| Arc::ptr_eq(&current, &cancellation))
                    && this.scanner().map(|scanner| Rc::as_ptr(&scanner) as usize) == Some(scanner_id)
                    && this.current_find_query() == query
                    && this.find_bar().map(|bar| bar.current_query()) == Some(query.clone()))
                {
                    return;
                }
                if let Some(results) = this.search_results() {
                    results.set_hits(hits);
                }
                if let Some(results) = this.search_results() {
                    results.set_is_searching(false);
                }
            });
        });
    }

    /// `cancelSiblingSearch()`.
    pub fn cancel_sibling_search(&self) {
        if let Some(token) = self.sibling_search_cancellation() {
            token.cancel();
        }
        self.set_sibling_search_cancellation(None);
        self.set_sibling_search_generation(self.sibling_search_generation().wrapping_add(1));
        if let Some(results) = self.search_results() {
            results.set_is_searching(false);
        }
    }

    /// A scanner replacement changes the directory scope. Retire work tied to
    /// the old instance and rerun the visible query against the new file
    /// list.
    pub fn sibling_search_scanner_did_change(&self) {
        if self.scanner().is_none() {
            if let Some(token) = self.sibling_search_cancellation() {
                token.cancel();
            }
            self.set_sibling_search_cancellation(None);
            self.set_sibling_search_generation(self.sibling_search_generation().wrapping_add(1));
            if let Some(results) = self.search_results() {
                results.set_searched_file_count(0);
            }
            if let Some(results) = self.search_results() {
                results.set_hits(Vec::new());
            }
            if let Some(results) = self.search_results() {
                results.set_is_searching(false);
            }
            return;
        }
        if !self.sibling_search_active() {
            return;
        }
        // A document hop clears the local FindSession while retaining the
        // visible field. Run the normal Find path so both authorities adopt
        // the field's query before the sibling result is allowed to publish.
        let query = self.find_bar().map(|bar| bar.current_query()).unwrap_or_default();
        self.run_find(query, false, true);
    }
}

/// `NSError(domain:code:userInfo: [NSLocalizedDescriptionKey: …])`.
fn error_with_description(domain: &str, code: isize, description: &str) -> Retained<NSError> {
    let key = unsafe { NSLocalizedDescriptionKey };
    let value = NSString::from_str(description);
    let user_info = NSDictionary::from_slices(&[key], &[value.as_ref() as &AnyObject]);
    unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(domain), code, Some(&user_info)) }
}

/// `CocoaError(code).localizedDescription`.
pub fn cocoa_error_description(code: isize) -> String {
    let error = unsafe { NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, code, None) };
    error.localizedDescription().to_string()
}

// MARK: - Plain-text rendering (§9.5 "copy with all markup stripped")

/// `enum PlainTextRenderer`.
pub struct PlainTextRenderer;

impl PlainTextRenderer {
    /// `render(_:range:)`.
    pub fn render(document: &ParsedDocument, range: NSRange) -> String {
        let mut out = String::new();
        document.root.walk(&mut |block| {
            if !(ns_intersection_range(block.range, range).length > 0) {
                return;
            }
            match &block.content {
                BlockContent::Heading { .. } | BlockContent::Paragraph => {
                    let text = document.substring(ns_intersection_range(block.content_range, range));
                    out += &Self::strip_inline_markers(&text);
                    out += "\n\n";
                }
                BlockContent::CodeBlock { content_range, .. } => {
                    out += &document.substring(ns_intersection_range(*content_range, range));
                    out += "\n\n";
                }
                BlockContent::ListItem { ordinal, checkbox } => {
                    let bullet =
                        ordinal.map(|ordinal| format!("{ordinal}. ")).unwrap_or_else(|| "\u{2022} ".to_owned());
                    let tick = checkbox
                        .as_ref()
                        .map(|checkbox| if checkbox.is_checked { "[x] " } else { "[ ] " })
                        .unwrap_or("");
                    out += &bullet;
                    out += tick;
                }
                _ => {}
            }
        });
        upleft_swift_text::trim_whitespaces_and_newlines(&out).to_owned()
    }

    /// Strips emphasis, code, and link syntax without a second parse — the
    /// clipboard does not need a perfect AST, it needs the words. The
    /// replacements are Foundation's, as `String.replacingOccurrences` is.
    fn strip_inline_markers(text: &str) -> String {
        let mut out = NSString::from_str(text);
        let empty = NSString::from_str("");
        for marker in ["***", "**", "___", "__", "~~", "*", "_", "`"] {
            out = out.stringByReplacingOccurrencesOfString_withString(&NSString::from_str(marker), &empty);
        }
        // [label](target) -> label
        out = replacing_regex(&out, r"\[([^\]]*)\]\([^)]*\)", "$1");
        // [[target|label]] -> label, [[target]] -> target
        out = replacing_regex(&out, r"\[\[([^\]|]*)\|([^\]]*)\]\]", "$2");
        out = replacing_regex(&out, r"\[\[([^\]]*)\]\]", "$1");
        out.to_string()
    }
}

/// `replacingOccurrences(of:with:options: .regularExpression)`.
fn replacing_regex(text: &NSString, pattern: &str, template: &str) -> Retained<NSString> {
    text.stringByReplacingOccurrencesOfString_withString_options_range(
        &NSString::from_str(pattern),
        &NSString::from_str(template),
        NSStringCompareOptions::RegularExpressionSearch,
        FoundationRange::new(0, text.length()),
    )
}

// MARK: - Print and PDF

/// Print and PDF go through an `NSAttributedString` built from the exported
/// HTML. That keeps one stylesheet — the print one — describing paper
/// output, rather than a second layout pass that would drift from the
/// export (§9.5).
pub struct PrintRenderer;

impl PrintRenderer {
    fn attributed_string(html: &str) -> Option<Retained<objc2_foundation::NSAttributedString>> {
        let data = NSData::with_bytes(html.as_bytes());
        let encoding = NSNumber::new_usize(NSUTF8StringEncoding);
        let options: Retained<NSDictionary<NSAttributedStringDocumentReadingOptionKey, AnyObject>> = unsafe {
            NSDictionary::from_slices(
                &[NSDocumentTypeDocumentOption, NSCharacterEncodingDocumentOption],
                &[NSHTMLTextDocumentType.as_ref() as &AnyObject, encoding.as_ref() as &AnyObject],
            )
        };
        unsafe {
            objc2_foundation::NSAttributedString::initWithData_options_documentAttributes_error(
                objc2_foundation::NSAttributedString::alloc(),
                &data,
                &options,
                None,
            )
        }
        .ok()
    }

    /// `print(html:jobTitle:)`.
    pub fn print(html: &str, job_title: &str, mtm: MainThreadMarker) {
        let Some(attributed) = Self::attributed_string(html) else { return };
        let text_view = NSTextView::initWithFrame(NSTextView::alloc(mtm), rect(0.0, 0.0, 468.0, 648.0));
        if let Some(storage) = unsafe { text_view.textStorage() } {
            storage.setAttributedString(&attributed);
        }

        let info = NSPrintInfo::sharedPrintInfo();
        info.setTopMargin(56.0);
        info.setBottomMargin(56.0);
        info.setLeftMargin(56.0);
        info.setRightMargin(56.0);
        info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);

        let operation = NSPrintOperation::printOperationWithView_printInfo(&text_view, &info);
        operation.setJobTitle(Some(&NSString::from_str(job_title)));
        operation.setShowsPrintPanel(true);
        operation.runOperation();
    }

    /// `writePDF(html:to:)`.
    pub fn write_pdf(html: &str, url: &objc2_foundation::NSURL, mtm: MainThreadMarker) -> bool {
        let Some(attributed) = Self::attributed_string(html) else { return false };
        let text_view = NSTextView::initWithFrame(NSTextView::alloc(mtm), rect(0.0, 0.0, 468.0, 648.0));
        if let Some(storage) = unsafe { text_view.textStorage() } {
            storage.setAttributedString(&attributed);
        }

        let info: Retained<NSPrintInfo> = unsafe { objc2::msg_send![&*NSPrintInfo::sharedPrintInfo(), copy] };
        info.setJobDisposition(unsafe { NSPrintSaveJob });
        let dictionary: Retained<NSMutableDictionary<NSString, AnyObject>> =
            unsafe { objc2::msg_send![&*info, dictionary] };
        unsafe {
            let _: () = objc2::msg_send![&*dictionary, setObject: url, forKey: NSPrintJobSavingURL];
        }
        info.setTopMargin(56.0);
        info.setBottomMargin(56.0);
        info.setLeftMargin(56.0);
        info.setRightMargin(56.0);

        let operation = NSPrintOperation::printOperationWithView_printInfo(&text_view, &info);
        operation.setShowsPrintPanel(false);
        operation.setShowsProgressPanel(false);
        operation.runOperation()
    }
}
