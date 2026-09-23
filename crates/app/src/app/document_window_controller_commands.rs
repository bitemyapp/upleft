//! Port of `App/DocumentWindowController+Commands.swift`: command dispatch.
//! One switch over the `Command` table (§7.2) — the menu, the keyboard layer,
//! and the toolbar all arrive here, so a command behaves identically however
//! it was invoked.
//!
//! The Objective-C entry points of this extension (`validateMenuItem:`,
//! `performDownrightCommand:`) are declared in
//! `document_window_controller.rs`'s `define_class!` and forward to the
//! methods below.
//!
//! `(NSApp.delegate as? AppDelegate)` is [`app_delegate`]: the application's
//! delegate, downcast by Objective-C class.

use std::collections::HashSet;
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSControlStateValueOff, NSControlStateValueOn,
    NSEvent, NSEventModifierFlags, NSFont, NSFontWeightRegular, NSMenuItem, NSPasteboard, NSPasteboardTypeString,
    NSStandardKeyBindingResponding, NSTextField, NSWorkspace,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};
use upleft_core::list_editing::ListEditing;
use upleft_core::restructure::{MoveDirection, Restructure};
use upleft_core::swift_text::ns::{NSStringExt, foundation, utf16};
use upleft_core::tidy::TidyDocument;
use upleft_core::{ListConversion, ListSortOrder, NSRange, TextEdit, TidyRule, ZoomLevel};
use upleft_render::render_contracts::SourceFocus;
use upleft_render::swift_compat::{smax, smin};
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_swift_text as swift_text;

use crate::app::app_delegate::AppDelegate;
use crate::app::document_window_controller::DocumentWindowController;
use crate::app::document_window_controller_support::CopyFlavour;
use crate::app::main_menu::{CommandResponder, MainMenu};
use crate::app::version_timeline_window_controller::VersionTimelineWindowController;
use crate::panels::document_quick_look::QuickLookHost;
use crate::security::document_trust::TrustEffect;
use crate::support::commands::{Command, CommandContext, CommandScope, KeyBinding};
use crate::support::find_engine::FindQuery;
use crate::support::keybindings::KeybindingStore;
use crate::support::preferences::Preferences;
use crate::updater::update_coordinator::UpdateCoordinator;

/// `NSApp.delegate as? AppDelegate`.
pub(crate) fn app_delegate(mtm: MainThreadMarker) -> Option<Retained<AppDelegate>> {
    let delegate = NSApplication::sharedApplication(mtm).delegate()?;
    let object: &AnyObject = delegate.as_ref();
    object.downcast_ref::<AppDelegate>().map(|delegate| delegate.retain())
}

/// Menu state comes from the command table's preconditions, never from a
/// second switch that can drift out of step with it.
impl DocumentWindowController {
    /// `validateMenuItem(_:)` (`NSMenuItemValidation`).
    pub fn validate_menu_item(&self, menu_item: &NSMenuItem) -> bool {
        if let Some(command) = MainMenu::command(menu_item) {
            menu_item.setState(if self.command_state(command) { NSControlStateValueOn } else { NSControlStateValueOff });
        }
        MainMenu::validate(menu_item, &self.command_context())
    }
}

impl CommandResponder for DocumentWindowController {
    fn perform_downright_command(&self, sender: Option<&AnyObject>) {
        DocumentWindowController::perform_downright_command(self, sender);
    }
}

impl DocumentWindowController {
    fn commands_mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    /// `@objc performDownrightCommand(_:)` (`CommandResponder`).
    pub fn perform_downright_command(&self, sender: Option<&AnyObject>) {
        let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else { return };
        let Some(command) = MainMenu::command(item) else { return };
        self.perform(command);
    }

    /// `commandContext`: the facts this window contributes to command
    /// validation.
    pub fn command_context(&self) -> CommandContext {
        CommandContext {
            has_document: true,
            document_has_file: self.markdown_document().url().is_some(),
            has_selection: self.container_text_view().source_selected_range().length > 0,
            can_check_for_updates: UpdateCoordinator::shared(self.commands_mtm()).can_check_for_updates(),
            has_unsaved_changes: self.markdown_document().is_dirty(),
            has_find_query: !self.find_session().borrow().query().is_empty(),
            can_go_back: self.jump_history().borrow().can_go_back(),
            can_go_forward: self.jump_history().borrow().can_go_forward(),
            is_speaking: self.is_speaking_document(),
            caret_is_in_table: self.caret_is_in_table(),
            has_change_marks: !self.markdown_document().changes().decorated_marks().is_empty(),
            has_quick_look_target: self.has_quick_look_target(),
        }
    }

    /// Wires the text view's `keyDown` into the binding store — the one
    /// layer that previously had zero call sites, leaving the bare 1–5 zoom
    /// keys, the read-mode `space`/`n`/`p` keys, `⌥↓`/`⌥↑` change jumps, and
    /// the vim layer unreachable. The scope mirrors the surface's real state:
    /// an editable surface has a caret, so bare single-letter read bindings
    /// defer to typing there and only modified chords run; a read-only
    /// surface uses `.read`, where those bare keys are exactly what §7.2
    /// spends them on.
    pub fn wire_key_event_handler(&self, text_view: &MarkdownTextView) {
        let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::new(self);
        // Swift's closure captures `textView` strongly, and the text view
        // stores the closure: the cycle is Downright's, kept on purpose.
        let text_view_ref = text_view.retain();
        text_view.set_key_event_handler(Some(std::rc::Rc::new(move |event: &NSEvent| {
            let Some(this) = weak.load() else { return false };
            this.dispatch_key_event(event, &text_view_ref)
        })));
    }

    fn dispatch_key_event(&self, event: &NSEvent, text_view: &MarkdownTextView) -> bool {
        let scope = if text_view.source_focus() != SourceFocus::None {
            CommandScope::Source
        } else if !text_view.isEditable() {
            CommandScope::Read
        } else {
            CommandScope::Live
        };
        // The store's guard is released before anything runs: a command may
        // read the store again (menu key equivalents, the palette).
        let command = KeybindingStore::shared().command_for_event(event, scope);
        let Some(command) = command else { return false };

        // An editable surface has a caret, so an unmodified key is input, not
        // a command (§7.2's whole premise). Without this guard the read-layer
        // extras that share the default scope — notably the `[`/`]` change
        // navigation — would swallow literal typing. Read mode has no caret,
        // so bare keys (space, 1–5, n, p, j/k/g/G, `[`/`]`) fire there.
        //
        // Tab is the exception: it is bound to Indent/Outdent in `.live`, and
        // those are the keys every editor uses for list nesting. It stays
        // safe because `indentSelection` reports failure when the caret is
        // not in a list item, and the text view then inserts the literal tab.
        if scope != CommandScope::Read
            && (event.modifierFlags()
                & (NSEventModifierFlags::Command | NSEventModifierFlags::Option | NSEventModifierFlags::Control))
                .is_empty()
            && KeyBinding::key_for_event(event).as_deref() != Some("tab")
        {
            return false;
        }

        match command {
            Command::ScrollDown => {
                // SAFETY: a nil sender, as Swift passes.
                unsafe { text_view.scrollLineDown(None) };
                true
            }
            Command::ScrollUp => {
                // SAFETY: a nil sender, as Swift passes.
                unsafe { text_view.scrollLineUp(None) };
                true
            }
            Command::PageDown => {
                // SAFETY: a nil sender, as Swift passes.
                unsafe { text_view.scrollPageDown(None) };
                true
            }
            Command::PageUp => {
                // SAFETY: a nil sender, as Swift passes.
                unsafe { text_view.scrollPageUp(None) };
                true
            }
            _ => self.perform(command),
        }
    }

    /// `perform(_:)`: runs a command and reports whether it was handled.
    pub fn perform(&self, command: Command) -> bool {
        match command {
            // MARK: Modes
            Command::SourceMode => {
                if self.container_text_view().source_focus() == SourceFocus::None {
                    self.container_text_view().focus_entire_source();
                } else {
                    self.container_text_view().clear_source_focus();
                }
            }
            Command::SplitView => self.toggle_split_view(),
            Command::PinWindow => self.toggle_pin(),
            Command::FocusMode => self.toggle_focus_mode(),
            Command::TypewriterScrolling => {
                Preferences::shared().update(|values| values.typewriter_scrolling = !values.typewriter_scrolling)
            }
            Command::StatusBar => Preferences::shared().update(|values| values.show_status_bar = !values.show_status_bar),

            // MARK: Panels
            Command::TaskPanel => self.toggle_task_panel(),
            Command::VersionTimeline => self.show_version_timeline(),
            Command::FrontMatterEditor => self.show_front_matter_editor(None),
            Command::TableEditor => self.present_table_editor(),
            Command::AssetDoctor => self.toggle_asset_doctor_panel(),
            Command::CommandPalette => self.show_command_palette(),
            Command::DocumentLens => self.toggle_document_lens_panel(),
            Command::ReaderProfiles => self.show_reader_profiles(),
            Command::DocumentHealth => self.toggle_document_health_panel(),
            Command::RenderTargets => self.toggle_render_targets_panel(),
            Command::VisualDebugger => self.toggle_visual_debugger_panel(),
            Command::ReviewPanel => self.show_review_panel(),
            Command::Workspace => self.toggle_workspace_sidebar(),
            Command::LocalAi => self.show_local_ai_panel(),

            // MARK: Navigation
            Command::NextHeading => self.jump_heading(true),
            Command::PreviousHeading => self.jump_heading(false),
            Command::NextChange => self.jump_change(true),
            Command::PreviousChange => self.jump_change(false),
            Command::MarkChangesReviewed => self.mark_changes_reviewed(),
            Command::FollowLinkAtCaret => return self.container_text_view().activate_link_at_caret(),
            Command::NextLink => return self.container_text_view().move_to_link(true),
            Command::PreviousLink => return self.container_text_view().move_to_link(false),
            Command::DocumentStart => self.jump(0, "Top", true),
            Command::DocumentEnd => self.jump(self.markdown_document().parsed().length, "End", true),
            Command::GoBack => self.go_back(),
            Command::GoForward => self.go_forward(),
            Command::GoToLine => self.go_to_line(),
            // Scrolling is geometry the text view owns, but the Navigate menu
            // items arrive here, so route them rather than dropping them on
            // the floor. (SAFETY, for the four calls: a nil sender, as Swift
            // passes.)
            Command::ScrollDown => unsafe { self.container_text_view().scrollLineDown(None) },
            Command::ScrollUp => unsafe { self.container_text_view().scrollLineUp(None) },
            Command::PageDown => unsafe { self.container_text_view().scrollPageDown(None) },
            Command::PageUp => unsafe { self.container_text_view().scrollPageUp(None) },

            // MARK: Structural zoom (§5.2)
            Command::ZoomLevel1 => self.set_zoom(ZoomLevel::H1),
            Command::ZoomLevel2 => self.set_zoom(ZoomLevel::H2),
            Command::ZoomLevel3 => self.set_zoom(ZoomLevel::Headings),
            Command::ZoomLevel4 => self.set_zoom(ZoomLevel::Skeleton),
            Command::ZoomLevel5 => self.set_zoom(ZoomLevel::Everything),
            Command::ZoomIn => self.set_zoom(
                ZoomLevel::from_raw_value(5.min(self.container_text_view().zoom_level().raw_value() + 1))
                    .unwrap_or(ZoomLevel::Everything),
            ),
            Command::ZoomOut => self.set_zoom(
                ZoomLevel::from_raw_value(1.max(self.container_text_view().zoom_level().raw_value() - 1))
                    .unwrap_or(ZoomLevel::H1),
            ),

            // MARK: Find (§9.4)
            Command::Find => self.show_find_bar(false, None),
            Command::FindReplace => self.show_find_bar(true, None),
            Command::FindNext => self.advance_find(true),
            Command::FindPrevious => self.advance_find(false),
            Command::FindInSiblings => self.show_sibling_search(),
            Command::UseSelectionForFind => self.use_selection_for_find(),

            // MARK: Restructuring (§9.2)
            Command::PromoteHeading => self.restructure_heading(true),
            Command::DemoteHeading => self.restructure_heading(false),
            Command::HeadingLevel1 => self.set_heading_level(1),
            Command::HeadingLevel2 => self.set_heading_level(2),
            Command::HeadingLevel3 => self.set_heading_level(3),
            Command::HeadingLevel4 => self.set_heading_level(4),
            Command::HeadingLevel5 => self.set_heading_level(5),
            Command::HeadingLevel6 => self.set_heading_level(6),
            Command::HeadingToBody => self.convert_heading_to_body(),
            Command::MoveBlockUp => self.move_block(MoveDirection::Up),
            Command::MoveBlockDown => self.move_block(MoveDirection::Down),
            Command::FoldSection => self.fold_current_section(true),
            Command::UnfoldSection => self.fold_current_section(false),
            Command::FoldAll => self.set_all_folds(true),
            Command::UnfoldAll => self.set_all_folds(false),
            Command::ConvertToParagraph => self.convert_selection(ListConversion::Paragraph),
            Command::ConvertToBulletList => self.convert_selection(ListConversion::BulletList),
            Command::ConvertToNumberedList => self.convert_selection(ListConversion::NumberedList),
            Command::ConvertToTaskList => self.convert_selection(ListConversion::TaskList),
            Command::ConvertToBlockquote => self.convert_selection(ListConversion::Blockquote),
            Command::SortListAlphabetically => self.sort_list(ListSortOrder::Alphabetical),
            Command::SortListByState => self.sort_list(ListSortOrder::UncheckedFirst),
            Command::InsertTableOfContents => self.insert_table_of_contents(),
            Command::TidyDocument => self.show_tidy_sheet(),

            // MARK: Editing (§6.4)
            Command::ToggleBold => self.wrap_selection("**", "Bold"),
            Command::ToggleItalic => self.wrap_selection("_", "Italic"),
            Command::ToggleStrikethrough => self.wrap_selection("~~", "Strikethrough"),
            Command::ToggleInlineCode => self.wrap_selection("`", "Inline Code"),
            Command::InsertLink => self.insert_link(),
            // Report whether the caret was actually in a list item, so Tab
            // can fall through to a literal tab when it was not.
            Command::IndentList => return self.indent_selection(false),
            Command::OutdentList => return self.indent_selection(true),
            Command::ToggleTaskAtCaret => self.toggle_task_at_caret(),

            // MARK: Files
            Command::Save => {
                let _ = self.save_document();
            }
            Command::SaveAs => self.save_as(),
            Command::Close => {
                if let Some(window) = self.window() {
                    window.performClose(None);
                }
            }
            Command::RevealInFinder => {
                let Some(url) = self.markdown_document().url() else { return true };
                let target = url.clone();
                self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, Rc::new(move || {
                    let urls = NSArray::from_retained_slice(&[target.to_nsurl()]);
                    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
                }));
            }
            // Reports whether anything was previewed, so a Quick Look aimed
            // at nothing can fall back the way the caller expects rather than
            // silently swallowing the key.
            Command::QuickLook => return self.quick_look_at_selection(),
            Command::OpenInEditor => {
                let Some(url) = self.markdown_document().url() else { return true };
                let target = url.clone();
                self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &url, Rc::new(move || {
                    Preferences::shared().values().external_editor.open(&target, None);
                }));
            }

            // MARK: Copy and export (§9.5)
            Command::CopyAsMarkdown => self.copy(CopyFlavour::Markdown),
            Command::CopyAsRichText => self.copy(CopyFlavour::RichText),
            Command::CopyAsPlainText => self.copy(CopyFlavour::Plain),
            Command::CopySection => self.copy_current_section(),
            Command::CopySectionLink => self.copy_section_link(None),
            Command::PrintDocument => self.print_document(),
            Command::ExportHtml => self.export_html(),
            Command::ExportPdf => self.export_pdf(),
            Command::ExportSelectionAsImage => self.export_selection_as_image(),
            Command::Share => self.share_document(),
            Command::ShareAsPdf => self.share_document_as_pdf(),
            Command::IncreaseTextSize => self.adjust_text_size(1.0),
            Command::DecreaseTextSize => self.adjust_text_size(-1.0),
            Command::ResetTextSize => Preferences::shared().update(|values| values.text_size_adjustment = 0.0),
            Command::SpeakDocument => self.speak_selection_or_document(),
            Command::StopSpeaking => self.stop_speaking(),

            // MARK: Application-level
            Command::NewDocument
            | Command::Open
            | Command::Preferences
            | Command::ShowKeybindings
            | Command::ReloadTheme
            | Command::CompareFiles
            | Command::CheckForUpdates
            | Command::ToggleLightDark => {
                return app_delegate(self.commands_mtm())
                    .map(|delegate| delegate.handle_application_command(command))
                    .unwrap_or(false);
            }
        }
        true
    }

    // MARK: - Zoom

    fn set_zoom(&self, level: ZoomLevel) {
        // Headings hold their vertical position through the transition, so
        // the reader never loses their place (§5.2) — the text view anchors
        // on the nearest heading before relayout.
        self.set_shared_zoom(level);
    }

    // MARK: - Navigation helpers

    fn jump_heading(&self, forward: bool) {
        let offset = self.container_text_view().top_visible_offset();
        let parsed = self.markdown_document().parsed();
        let headings = &parsed.headings;
        let target = if forward {
            headings.iter().find(|heading| heading.range.location > offset + 1)
        } else {
            headings.iter().rev().find(|heading| heading.range.location < offset - 1)
        };
        let Some(target) = target else { return };
        self.jump(target.range.location, &target.title, true);
    }

    fn jump_change(&self, forward: bool) {
        let offset = self.container_text_view().top_visible_offset();
        let mark = if forward {
            self.markdown_document().changes().next(offset)
        } else {
            self.markdown_document().changes().previous(offset)
        };
        let Some(mark) = mark else { return };
        // Arriving is not reviewing. Marking visited here fired `onChange`,
        // which rebuilt the decorations without this mark — so the highlight
        // vanished at the exact moment the reader landed on it. Departure and
        // dwell are handled by `noteVisibleChangeMarks`.
        self.jump(mark.range.location, "Change", true);
    }

    fn show_version_timeline(&self) {
        if self.markdown_document().url().is_none() {
            return;
        }
        let controller = VersionTimelineWindowController::new(
            self.markdown_document(),
            self.current_style_sheet(),
            self.commands_mtm(),
        );
        // SAFETY: a nil sender, as Swift passes.
        unsafe { controller.showWindow(None) };
        self.retain_timeline(&controller);
    }

    // MARK: - Restructuring

    fn restructure_heading(&self, promote: bool) {
        self.markdown_document().ensure_parsed_current();
        let Some(index) = self.current_heading_index() else { return };
        let parsed = self.markdown_document().parsed();
        let edits = if promote {
            Restructure::promote_heading(&parsed, index as isize)
        } else {
            Restructure::demote_heading(&parsed, index as isize)
        };
        if edits.is_empty() {
            return;
        }
        self.apply_in_place_document_edits(&edits, if promote { "Promote Heading" } else { "Demote Heading" }, None);
    }

    fn set_heading_level(&self, level: isize) {
        self.markdown_document().ensure_parsed_current();
        let Some(index) = self.current_heading_index() else { return };
        let edits = Restructure::set_heading_level(&self.markdown_document().parsed(), index as isize, level);
        if edits.is_empty() {
            return;
        }
        let anchor = self.markdown_document().parsed().headings[index as usize].range.location;
        self.apply_in_place_document_edits(&edits, &format!("Set Heading {level}"), Some(anchor));
    }

    fn convert_heading_to_body(&self) {
        self.markdown_document().ensure_parsed_current();
        let Some(index) = self.current_heading_index() else { return };
        let edits = Restructure::heading_to_body_text(&self.markdown_document().parsed(), index as isize);
        if edits.is_empty() {
            return;
        }
        self.apply_in_place_document_edits(&edits, "Body Text", None);
    }

    /// Structural controls mutate storage outside
    /// `MarkdownTextView.performSourceEdit`. Leaving the old display map alive
    /// until the async parser runs makes TextKit briefly fall back to raw
    /// paragraphs, which is the visible page teleport. Lock each pane's pixel
    /// camera and publish the matching parse before this event can draw that
    /// inconsistent state.
    pub fn apply_in_place_document_edits(&self, edits: &[TextEdit], action_name: &str, anchor_offset: Option<isize>) {
        if edits.is_empty() {
            return;
        }
        let viewport_repairs: Vec<Box<dyn Fn()>> = self
            .document_panes()
            .iter()
            .map(|pane| match anchor_offset {
                Some(offset) => Box::new(pane.text_view().make_viewport_repair_at(offset)) as Box<dyn Fn()>,
                None => Box::new(pane.text_view().make_viewport_repair()) as Box<dyn Fn()>,
            })
            .collect();
        for pane in self.document_panes() {
            pane.text_view().preserve_viewport_on_next_document_update();
        }
        self.markdown_document().apply(edits, action_name, None);
        for repair in &viewport_repairs {
            repair();
        }
    }

    fn move_block(&self, direction: MoveDirection) {
        self.markdown_document().ensure_parsed_current();
        let offset = self.caret_offset();
        let edits = Restructure::move_block(&self.markdown_document().parsed(), offset, direction);
        self.markdown_document().apply(
            &edits,
            if direction == MoveDirection::Up { "Move Block Up" } else { "Move Block Down" },
            None,
        );
    }

    fn convert_selection(&self, conversion: ListConversion) {
        self.markdown_document().ensure_parsed_current();
        let edits = Restructure::convert(&self.markdown_document().parsed(), self.selection_range(), conversion);
        self.markdown_document().apply(&edits, &format!("Convert to {}", conversion.title()), None);
    }

    fn sort_list(&self, order: ListSortOrder) {
        self.markdown_document().ensure_parsed_current();
        let edits = Restructure::sort_list(&self.markdown_document().parsed(), self.caret_offset(), order);
        self.markdown_document().apply(&edits, "Sort List", None);
    }

    fn insert_table_of_contents(&self) {
        self.markdown_document().ensure_parsed_current();
        let toc = Restructure::table_of_contents(&self.markdown_document().parsed(), 3);
        if toc.is_empty() {
            return;
        }
        let insertion =
            self.markdown_document().parsed().front_matter.as_ref().map_or(0, |front_matter| front_matter.range.upper_bound());
        self.markdown_document().apply(
            &[TextEdit::new(NSRange::new(insertion, 0), toc, "Table of contents", None)],
            "Insert Table of Contents",
            None,
        );
    }

    fn fold_current_section(&self, fold: bool) {
        self.markdown_document().ensure_parsed_current();
        let Some(index) = self.current_heading_index() else { return };
        let slug = self.markdown_document().parsed().headings[index as usize].slug.clone();
        let text_view = self.container_text_view();
        let mut folds = text_view.folded_heading_slugs();
        if fold {
            folds.insert(slug);
        } else {
            folds.remove(&slug);
        }
        self.set_shared_folds(folds, Some(&text_view));
    }

    fn set_all_folds(&self, folded: bool) {
        self.markdown_document().ensure_parsed_current();
        let folds: HashSet<String> = if folded {
            self.markdown_document().parsed().headings.iter().map(|heading| heading.slug.clone()).collect()
        } else {
            HashSet::new()
        };
        self.set_shared_folds(folds, Some(&self.container_text_view()));
    }

    fn show_tidy_sheet(&self) {
        self.markdown_document().ensure_parsed_current();
        let edits = TidyDocument::plan(&self.markdown_document().parsed());
        if edits.is_empty() {
            let alert = NSAlert::new(self.commands_mtm());
            alert.setMessageText(&NSString::from_str("Nothing to tidy"));
            // Swift's text, a rename artifact included.
            alert.setInformativeText(&NSString::from_str(
                "This markdownDocument already follows every rule Tidy checks.",
            ));
            alert.runModal();
            return;
        }
        self.present_tidy_sheet(&edits);
    }

    // MARK: - Editing helpers

    fn wrap_selection(&self, marker: &str, name: &str) {
        let range = self.selection_range();
        let text = utf16(&self.markdown_document().text());
        let selected = if range.length > 0 { text.substring(range) } else { String::new() };
        // Toggling off is the same operation read backwards, which keeps ⌘B
        // on already-bold text doing what everybody expects.
        let marker_count = swift_text::count(marker);
        let replacement = if !selected.is_empty()
            && swift_text::has_prefix(&selected, marker)
            && swift_text::has_suffix(&selected, marker)
            && swift_text::count(&selected) > marker_count * 2
        {
            swift_text::drop_last(swift_text::drop_first(&selected, marker_count), marker_count).to_owned()
        } else if selected.is_empty() {
            format!("{marker}{marker}")
        } else {
            format!("{marker}{selected}{marker}")
        };
        self.apply_in_place_document_edits(&[TextEdit::new(range, replacement, name, None)], name, None);
        if selected.is_empty() {
            self.container_text_view()
                .set_source_selected_ranges(&[NSRange::new(range.location + swift_text::utf16_count(marker), 0)]);
        }
    }

    fn insert_link(&self) {
        // Deliberately NOT `selection_range()`. That helper widens an empty
        // selection to the caret's whole block, which is right for ⌘B and
        // the convert commands and very wrong here: ⌘K with nothing selected
        // swallowed the entire paragraph into `[…]()`.
        let range = self.container_text_view().source_selected_range();
        let selected =
            if range.length > 0 { utf16(&self.markdown_document().text()).substring(range) } else { String::new() };
        // SAFETY: AppKit's immutable pasteboard type constant.
        let clipboard = NSPasteboard::generalPasteboard()
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|string| foundation::to_string(&string))
            .unwrap_or_default();
        // A URL already on the clipboard is the common case and turns ⌘K
        // into a one-keystroke operation (§6.4 smart paste, applied to ⌘K).
        let destination = if swift_text::has_prefix(&clipboard, "http") { clipboard } else { String::new() };
        let replacement = format!("[{selected}]({destination})");
        self.apply_in_place_document_edits(
            &[TextEdit::new(range, replacement.clone(), "Insert Link", None)],
            "Insert Link",
            None,
        );
        // Land the caret on whichever half the user still has to fill in.
        let end = range.location + swift_text::utf16_count(&replacement);
        let caret = if selected.is_empty() {
            range.location + 1 // inside the empty [ ]
        } else if destination.is_empty() {
            end - 1 // inside the empty ( )
        } else {
            end // both filled: carry on typing
        };
        self.container_text_view().set_source_selected_ranges(&[NSRange::new(caret, 0)]);
    }

    /// Returns false when the caret's line is not a list item, which is what
    /// lets Tab reach the text view and type a literal tab instead.
    fn indent_selection(&self, outdent: bool) -> bool {
        self.markdown_document().ensure_parsed_current();
        let text = utf16(&self.markdown_document().text());
        let line = text.line_range_for(self.selection_range());
        let edits = ListEditing::indent(&self.markdown_document().parsed(), line, outdent);
        if edits.is_empty() {
            return false;
        }
        // §6.4: indent and outdent renumber ordered lists automatically. The
        // rule only touches markers that genuinely disagree with their
        // position in their own list.
        self.markdown_document().apply(
            &edits,
            if outdent { "Outdent" } else { "Indent" },
            Some(&[TidyRule::OrderedListNumbers]),
        );
        true
    }

    fn toggle_task_at_caret(&self) {
        self.markdown_document().ensure_parsed_current();
        let offset = self.caret_offset();
        let parsed = self.markdown_document().parsed();
        let Some(task) =
            parsed.tasks.iter().find(|task| task.content_range.touches(offset) || task.mark_range.touches(offset))
        else {
            return;
        };
        self.markdown_document().toggle_task(task.mark_range.location);
    }

    /// Shows a small dialog to jump to a line number. The line number is
    /// one-based, matching what a reader sees in a text editor status bar.
    fn go_to_line(&self) {
        let mtm = self.commands_mtm();
        let text = utf16(&self.markdown_document().text());
        // `components(separatedBy: "\n")`: a literal split on the unit.
        let components: Vec<&[u16]> = text.split(|&unit| unit == 0x0A).collect();
        let total_lines = 1.max(if text.length() == 0 {
            1
        } else if text.substring_from(0.max(text.length() - 1)) == "\n" {
            components.len() as isize - 1
        } else {
            components.len() as isize
        });
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str("Go to Line"));
        alert.setInformativeText(&NSString::from_str(&format!("Enter a line number (1–{total_lines}):")));
        alert.setAlertStyle(NSAlertStyle::Informational);
        let input = NSTextField::initWithFrame(
            NSTextField::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 24.0)),
        );
        input.setPlaceholderString(Some(&NSString::from_str("Line number")));
        // SAFETY: AppKit's immutable weight constant.
        input.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(13.0, unsafe { NSFontWeightRegular })));
        alert.setAccessoryView(Some(&input));
        alert.addButtonWithTitle(&NSString::from_str("Go"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        alert.window().setInitialFirstResponder(Some(&input));
        if alert.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        let entered = foundation::to_string(&input.stringValue());
        let Some(line_number) = swift_text::parse_int(swift_text::trim_whitespaces(&entered)) else { return };
        if !(line_number > 0) {
            return;
        }
        let target_line = (line_number - 1).min(total_lines - 1);
        let mut offset: isize = 0;
        for line in components.iter().take(target_line.min(components.len() as isize).max(0) as usize) {
            offset += line.len() as isize + 1; // +1 for the newline
        }
        offset = offset.min(text.length());
        self.jump(offset, &format!("Line {line_number}"), true);
    }

    fn use_selection_for_find(&self) {
        let range = self.container_text_view().source_selected_range();
        if !(range.length > 0) {
            return;
        }
        let mut query = FindQuery::default();
        query.text = utf16(&self.markdown_document().text()).substring(range);
        self.show_find_bar(false, Some(query));
    }

    // MARK: - Text size

    /// `adjustTextSize(by:)`.
    pub fn adjust_text_size(&self, delta: CGFloat) {
        Preferences::shared().update(|values| {
            values.text_size_adjustment = smax(-4.0, smin(10.0, values.text_size_adjustment + delta));
        });
    }
}
