//! `// MARK: - Window lifecycle` of `DocumentWindowController.swift` and the
//! `NSWindowDelegate` extension: the save prompt, close, pinning, focus mode,
//! and the delegate callbacks.

use objc2::rc::Retained;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSWindow, NSWindowOcclusionState};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString, NSUndoManager};

use objc2::DefinedClass as _;
use objc2::MainThreadOnly as _;
use super::DocumentWindowController;
use crate::ai::markdown_document::SaveIntent;
use crate::support::preferences::Preferences;

/// `NSWindow.Level.floating` / `.normal`.
const FLOATING_WINDOW_LEVEL: isize = 3;
const NORMAL_WINDOW_LEVEL: isize = 0;

impl DocumentWindowController {
    // MARK: - Window lifecycle

    /// `confirmPendingChangesBeforeClose(markDiscardForWindowClose:)`.
    pub fn confirm_pending_changes_before_close(&self, mark_discard_for_window_close: bool) -> bool {
        self.ivars().discard_changes_on_close.set(false);
        // The prompt runs a modal loop during which main-queue work items
        // still drain. A pending autosave firing here would write the buffer
        // before the user has even answered "Save changes?", so retire it up
        // front.
        if let Some(item) = self.ivars().autosave_work_item.borrow().as_ref() {
            item.cancel();
        }
        let document = self.markdown_document();
        if !(document.is_dirty() && document.url().is_some()) {
            return true;
        }

        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(&format!("Save changes to {}?", document.display_name())));
        alert.setInformativeText(&NSString::from_str("Your changes will be lost if you don't save them."));
        alert.addButtonWithTitle(&NSString::from_str("Save"));
        alert.addButtonWithTitle(&NSString::from_str("Discard"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        let response = alert.runModal();
        if response == NSAlertFirstButtonReturn {
            self.save_document()
        } else if response == NSAlertSecondButtonReturn {
            self.ivars().discard_changes_on_close.set(mark_discard_for_window_close);
            self.ivars().implicit_save_suppressed.set(true);
            true
        } else {
            false
        }
    }

    /// `documentWillClose()`.
    pub fn document_will_close(&self) -> bool {
        if !(self.ivars().discard_changes_on_close.get()
            || !self.markdown_document().is_dirty()
            || self.save_document())
        {
            return false;
        }
        self.ivars().discard_changes_on_close.set(false);
        self.stop_speaking();
        if let Some(item) = self.ivars().derived_ui_refresh_work_item.borrow().as_ref() {
            item.cancel();
        }
        if let Some(item) = self.ivars().find_refresh_work_item.borrow().as_ref() {
            item.cancel();
        }
        self.cancel_sibling_search();
        if let Some(item) = self.ivars().autosave_work_item.borrow().as_ref() {
            item.cancel();
        }
        self.remove_focus_dimming_views(false);
        self.remove_floating_surface();
        let selection = self.primary_container().text_view().source_selected_range();
        let mut state = self.markdown_document().state();
        state.selection_location = selection.location;
        state.selection_length = selection.length;
        state.split_view_enabled = self.split_view_container().is_some();
        self.markdown_document().set_state(state);
        self.markdown_document().close();
        if let Some(observation) = self.ivars().theme_observation.borrow().as_ref() {
            observation.cancel();
        }
        // SAFETY: removes every registration this controller made.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
        true
    }

    /// `togglePin()`.
    pub fn toggle_pin(&self) {
        let pinned = !self.ivars().is_pinned.get();
        self.ivars().is_pinned.set(pinned);
        if let Some(window) = self.window() {
            window.setLevel(if pinned { FLOATING_WINDOW_LEVEL } else { NORMAL_WINDOW_LEVEL });
        }
    }

    /// `toggleFocusMode()`.
    pub fn toggle_focus_mode(&self) {
        Preferences::shared().update(|values| values.focus_mode = !values.focus_mode);
    }

    /// `applyFocusMode(_:animated:)`.
    pub fn apply_focus_mode(&self, enabled: bool, animated: bool) {
        if enabled {
            if !self.ivars().focus_mode_applied.get() {
                self.ivars().focus_mode_applied.set(true);
            }
            self.close_task_panel();
            if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
                toolbar.setVisible(false);
            }
            if let Some(band) = self.toolbar_glass_band() {
                band.setHidden(true);
            }
            self.density_gutter_view().setHidden(true);
            self.breadcrumb_view().setHidden(true);
            self.refresh_change_summary_top_inset();
            for pane in self.document_panes() {
                self.install_focus_dimming_view(&pane);
            }
            self.update_focus_dimming_views();
        } else {
            if !self.ivars().focus_mode_applied.get() {
                if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
                    toolbar.setVisible(true);
                }
                if let Some(band) = self.toolbar_glass_band() {
                    band.setHidden(false);
                }
                self.density_gutter_view().setHidden(false);
                self.breadcrumb_view().setHidden(false);
                self.refresh_change_summary_top_inset();
                return;
            }
            self.remove_focus_dimming_views(animated);
            if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
                toolbar.setVisible(true);
            }
            if let Some(band) = self.toolbar_glass_band() {
                band.setHidden(false);
            }
            self.density_gutter_view().setHidden(false);
            self.breadcrumb_view().setHidden(false);
            self.refresh_change_summary_top_inset();
            self.ivars().focus_mode_applied.set(false);
        }
        self.refresh_toolbar_selection_state();
    }

    // MARK: - Window delegate

    pub fn window_did_become_key(&self, _notification: &NSNotification) {
        self.restore_initial_reading_position_if_ready();
        self.restore_floating_panel_window();
        self.refresh_toolbar_selection_state();
    }

    /// Ordinary app switching is not dismissal. The surface keeps its state
    /// while another document is active, and its responder is restored when
    /// this window becomes key again.
    pub fn window_did_resign_key(&self, _notification: &NSNotification) {
        // Intentionally empty. Explicit close, Esc, the ring, or an outside
        // click owns dismissal; Cmd-Tab must not destroy work-in-progress.
    }

    /// `windowDidBecomeVisible(_:)`: not an `NSWindowDelegate` method, so
    /// AppKit never calls it; kept for parity.
    pub fn window_did_become_visible(&self, _notification: &NSNotification) {
        self.restore_initial_reading_position_if_ready();
    }

    pub fn window_will_return_undo_manager(&self, _window: &NSWindow) -> Option<Retained<NSUndoManager>> {
        let manager: &NSUndoManager = self.markdown_document().undo_manager();
        Some(objc2::Message::retain(manager))
    }

    pub fn window_did_resize(&self, _notification: &NSNotification) {
        // A resize reflows both presentations, so a swipe in flight is holding
        // stills that no longer describe the document. Ground it first.
        self.presentation_swipe().cancel_in_flight();
        self.history_swipe().cancel_in_flight();
        self.update_focus_dimming_views();
        self.refit_floating_surface(true);
    }

    pub fn window_did_enter_full_screen(&self, _notification: &NSNotification) {
        if !self.is_focus_mode_enabled()
            && let Some(toolbar) = self.window().and_then(|window| window.toolbar())
        {
            toolbar.setVisible(true);
        }
        self.resettle_document_surface();
    }

    /// Leaving full screen restores the chrome the transition owns, and —
    /// like entering — resettles the document.
    pub fn window_did_exit_full_screen(&self, _notification: &NSNotification) {
        if !self.is_focus_mode_enabled()
            && let Some(toolbar) = self.window().and_then(|window| window.toolbar())
        {
            toolbar.setVisible(true);
        }
        self.resettle_document_surface();
    }

    pub fn window_did_deminiaturize(&self, _notification: &NSNotification) {
        self.resettle_document_surface();
    }

    pub fn window_did_change_backing_properties(&self, _notification: &NSNotification) {
        self.resettle_document_surface();
    }

    /// Re-primes the document after a transition that changed the window's
    /// size or surface out from under TextKit.
    fn resettle_document_surface(&self) {
        self.root_view().layoutSubtreeIfNeeded();
        for pane in self.document_panes() {
            pane.layoutSubtreeIfNeeded();
            pane.text_view().resize_to_fit_content();
            pane.text_view().prepare_for_display();
            pane.text_view().setNeedsDisplay(true);
            pane.scroll_view().contentView().setNeedsDisplay(true);
        }
        self.update_breadcrumb_and_gutter();
    }

    pub fn window_should_close(&self, _sender: &NSWindow) -> bool {
        self.confirm_pending_changes_before_close(true)
    }

    pub fn window_will_close(&self, _notification: &NSNotification) {
        // The window is gone regardless of what torn down; deregistration
        // must always run or AppDelegate.`windowControllers` keeps a dead
        // controller that can never be removed.
        let _ = self.document_will_close();
        // `defer { onClose?() }`
        if let Some(on_close) = self.on_close() {
            on_close();
        }
    }

    pub fn window_did_change_occlusion_state(&self, _notification: &NSNotification) {
        if self.window().map(|window| window.occlusionState().contains(NSWindowOcclusionState::Visible)) != Some(false) {
            return;
        }
        // Occlusion is an implicit save, so it belongs to the autosave
        // feature: with the setting off (the default) a covered or
        // miniaturized window must never write, because an agent may be
        // editing the same file. A discarded buffer is equally ineligible.
        if !(Preferences::shared().values().autosave_enabled && !self.ivars().implicit_save_suppressed.get()) {
            return;
        }
        let _ = self.markdown_document().save_if_needed(SaveIntent::Normal);
    }
}
