//! Theme and preference observation, `applyStyleSheet`, the modes (§3.2)
//! and pane synchronisation of `DocumentWindowController.swift`.

use std::collections::HashSet;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSNotification, NSString};
use objc2_quartz_core::{CAMediaTiming, CATransition, kCATransitionFade};
use upleft_core::{NSRange, ZoomLevel};
use upleft_foundation::url::FileUrl;
use upleft_render::motion::{self, Curve};
use upleft_render::render_contracts::{RenderMode, SourceFocus};
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use objc2::DefinedClass as _;
use objc2_app_kit::NSAppearanceCustomization as _;
use super::{DocumentRootView, DocumentWindowController};
use crate::ai::path_resolver::PathResolver;
use crate::ai::sibling_scanner::SiblingScanner;
use crate::app::themed_window_appearance::ThemedWindowAppearance;
use crate::panels::appkit_support::downcast;
use crate::support::preferences::Preferences;

impl DocumentWindowController {
    /// `@objc private func accessibilityDisplayOptionsDidChange()`.
    pub(super) fn accessibility_display_options_did_change(&self) {
        let Some(window) = self.window() else { return };
        self.set_active_style_sheet(Self::make_style_sheet(ThemeStore::shared().current(), &window.effectiveAppearance()));
        self.apply_style_sheet();
    }

    /// `@objc private func preferencesDidChange()`.
    pub(super) fn preferences_did_change(&self) {
        let should_resolve_path_tokens = Preferences::shared().values().resolve_path_tokens;
        let path_resolution_changed = should_resolve_path_tokens != self.ivars().resolves_path_tokens.get();
        self.ivars().resolves_path_tokens.set(should_resolve_path_tokens);
        if let Some(window) = self.window() {
            window.apply_theme_appearance(&ThemeStore::shared().current());
        }
        let appearance = match self.window() {
            Some(window) => window.effectiveAppearance(),
            None => NSApplication::sharedApplication(self.mtm()).effectiveAppearance(),
        };
        self.set_active_style_sheet(Self::make_style_sheet(ThemeStore::shared().current(), &appearance));
        self.apply_style_sheet();
        self.apply_focus_mode(Preferences::shared().values().focus_mode, true);
        self.apply_status_bar_preference();
        self.rebuild_sibling_scanner_if_folders_changed();
        if path_resolution_changed {
            self.refresh_path_resolution_preference(should_resolve_path_tokens);
        }
    }

    fn refresh_path_resolution_preference(&self, enabled: bool) {
        if let Some(resolver) = self.path_resolver() {
            resolver.invalidate();
        }
        for pane in self.document_panes() {
            pane.text_view().invalidate_path_existence_cache();
        }
        if !enabled {
            // The delegate returns neutral/exists while disabled, so this
            // bounded pass removes any missing-path underline without I/O.
            self.ivars().is_clearing_disabled_path_state.set(true);
            self.clear_disabled_path_state();
            return;
        }
        self.ivars().is_clearing_disabled_path_state.set(false);
        if let Some(resolver) = self.path_resolver() {
            let tokens: Vec<_> =
                self.markdown_document().parsed().path_tokens.iter().map(|token| token.token.clone()).collect();
            let handle = self.handle();
            resolver.warm(&tokens, move || {
                let Some(this) = handle.load() else { return };
                if !this.ivars().resolves_path_tokens.get() {
                    return;
                }
                for pane in this.document_panes() {
                    pane.text_view().invalidate_path_existence_cache();
                }
                this.primary_container().text_view().refresh_path_existence(None);
                if let Some(split) = this.split_container() {
                    split.text_view().setNeedsDisplay(true);
                }
            });
        }
    }

    pub(super) fn clear_disabled_path_state(&self) {
        if self.ivars().resolves_path_tokens.get() {
            self.ivars().is_clearing_disabled_path_state.set(false);
            return;
        }
        for pane in self.document_panes() {
            pane.text_view().invalidate_path_existence_cache();
        }
        let weak = objc2::rc::Weak::new(self);
        self.primary_container().text_view().refresh_path_existence(Some(Box::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().resolves_path_tokens.get() {
                return;
            }
            this.ivars().is_clearing_disabled_path_state.set(false);
            if let Some(split) = this.split_container() {
                split.text_view().setNeedsDisplay(true);
            }
        })));
    }

    /// The status bar is opt-in (DESIGN.md's "Avoid" list names a permanent
    /// one), so its visibility follows the preference rather than the
    /// window's existence.
    pub fn apply_status_bar_preference(&self) {
        self.status_bar_view().set_is_visible(Preferences::shared().values().show_status_bar);
    }

    /// A rename is an identity change, not content to reconcile: the bytes
    /// are the same, the path is not.
    pub(super) fn adopt_renamed_file(&self, new_url: &FileUrl) {
        if let Some(window) = self.window() {
            window.setTitle(&NSString::from_str(&new_url.last_path_component()));
        }
        if let Some(window) = self.window() {
            window.setSubtitle(&NSString::from_str(&new_url.deleting_last_path_component().last_path_component()));
        }
        if let Some(window) = self.window() {
            window.setRepresentedURL(Some(&new_url.to_nsurl()));
        }
        self.configure_local_asset_access(self.primary_container().text_view(), Some(new_url));
        if let Some(split) = self.split_container() {
            self.configure_local_asset_access(split.text_view(), Some(new_url));
        }
        self.set_path_resolver(Some(PathResolver::new(Some(new_url))));
        self.set_scanner(Some(Rc::new(SiblingScanner::new(
            new_url,
            Preferences::shared().values().sibling_scan_directories,
        ))));
        self.dismiss_conflict_bar();
        self.show_change_summary(Some(&format!("Renamed to {}", new_url.last_path_component())));
    }

    /// The scanner is otherwise built only in `open(_:mode:)`, so editing the
    /// extra-folder list in Settings appeared to do nothing until the
    /// document was reopened.
    fn rebuild_sibling_scanner_if_folders_changed(&self) {
        let folders = Preferences::shared().values().sibling_scan_directories;
        let Some(url) = self.markdown_document().url() else { return };
        let unchanged = self
            .scanner()
            .map(|scanner| {
                let existing = scanner.extra_directories();
                existing.len() == folders.len()
                    && existing.iter().zip(&folders).all(|(a, b)| upleft_swift_text::str_eq(a, b))
            })
            .unwrap_or(false);
        if unchanged {
            return;
        }
        self.set_scanner(Some(Rc::new(SiblingScanner::new(&url, folders))));
    }

    /// `applyStyleSheet()`.
    pub fn apply_style_sheet(&self) {
        let sheet = self.active_style_sheet();
        self.primary_container().set_style_sheet(sheet.clone());
        if let Some(split) = self.split_container() {
            split.set_style_sheet(sheet.clone());
        }
        if let Some(split) = self.split_view_container() {
            split.set_style_sheet(sheet.clone());
        }
        self.breadcrumb_view().set_style_sheet(sheet.clone());
        self.density_gutter_view().set_style_sheet(sheet.clone());
        self.status_bar_view().set_style_sheet(sheet.clone());
        self.progress_ring().set_style_sheet(sheet.clone());
        if let Some(panel) = self.task_panel() {
            panel.set_style_sheet(sheet.clone());
        }
        if let Some(surface) = self.floating_surface() {
            surface.set_style_sheet(sheet.clone());
        }
        if let Some(bar) = self.find_bar() {
            bar.set_style_sheet(sheet.clone());
        }
        if let Some(inspector) = self.search_inspector() {
            inspector.set_style_sheet(sheet.clone());
        }
        if let Some(inspector) = self.history_inspector() {
            inspector.set_style_sheet(sheet.clone());
        }
        if let Some(bar) = self.conflict_bar() {
            bar.set_style_sheet(sheet.clone());
        }
        if let Some(bar) = self.change_summary_bar() {
            bar.set_style_sheet(sheet.clone());
        }
        if let Some(panel) = self.search_results() {
            panel.set_style_sheet(sheet.clone());
        }
        if let Some(editor) = self.front_matter_editor() {
            editor.set_style_sheet(sheet.clone());
        }
        if let Some(panel) = self.asset_doctor_panel() {
            panel.set_style_sheet(sheet.clone());
        }
        // The inspector header is chrome like any other panel's; it never
        // followed the theme before because nothing assigned it one.
        if let Some(host) = self.inspector_host() {
            host.set_style_sheet(sheet.clone());
        }
        if let Some(root) = downcast::<DocumentRootView>(&self.root_view()) {
            root.set_background_color(&sheet.background);
        }
        if let Some(band) = self.toolbar_glass_band() {
            band.set_style_sheet(sheet.clone());
        }
        if let Some(window) = self.window() {
            window.setBackgroundColor(Some(&sheet.background));
        }
        self.apply_render_configuration();
    }

    pub(super) fn apply_render_configuration(&self) {
        let configuration = self.render_configuration();
        for pane in self.document_panes() {
            if pane.text_view().configuration() != configuration {
                pane.text_view().set_configuration(configuration.clone());
            }
        }
    }

    // MARK: - Modes (§3.2)

    /// `applyMode(_:)`.
    pub fn apply_mode(&self, new_mode: RenderMode) {
        let new_mode = new_mode.normalized_for_editing();
        self.set_mode(new_mode);
        let reduce_motion = self.active_style_sheet().reduce_motion;
        for pane in self.document_panes() {
            let text_view = pane.text_view();
            if text_view.mode() == new_mode {
                continue;
            }
            if !reduce_motion && text_view.window().is_some() {
                text_view.setWantsLayer(true);
                let key = NSString::from_str("mode-crossfade");
                if let Some(layer) = text_view.layer() {
                    layer.removeAnimationForKey(&key);
                }
                let transition = CATransition::new();
                // SAFETY: `kCATransitionFade` is an immutable global.
                transition.setType(unsafe { kCATransitionFade });
                transition.setDuration(motion::STANDARD);
                transition.setTimingFunction(Some(&motion::timing(Curve::Decelerate)));
                if let Some(layer) = text_view.layer() {
                    layer.addAnimation_forKey(&transition, Some(&key));
                }
            }
            text_view.set_mode(new_mode);
        }
        // Never persist a transient raw-source presentation.
        let mut state = self.markdown_document().state();
        state.mode = RenderMode::Live;
        self.markdown_document().set_state(state);
        self.refresh_source_focus_toolbar();
        if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
            toolbar.validateVisibleItems();
        }
    }

    /// Share document presentation state without making one pane's caret or
    /// camera overwrite the other pane's. Split panes are two editors over
    /// one buffer; selection and scroll are local interaction state.
    pub fn synchronize_panes(&self, source: &MarkdownTextView, selection: bool, viewport: bool) {
        if self.split_container().is_none() || self.ivars().is_synchronizing_panes.get() {
            return;
        }
        self.ivars().is_synchronizing_panes.set(true);
        let shared_selection = if selection { Some(source.source_selected_ranges()) } else { None };
        let shared_scroll_offset = if viewport { Some(source.top_visible_offset()) } else { None };
        for pane in self.document_panes() {
            let text_view = pane.text_view();
            if std::ptr::eq(&**text_view, source) {
                continue;
            }
            if text_view.configuration() != source.configuration() {
                text_view.set_configuration(source.configuration());
            }
            if text_view.mode() != source.mode() {
                text_view.set_mode(source.mode());
            }
            if text_view.zoom_level() != source.zoom_level() {
                text_view.set_zoom_level(source.zoom_level());
            }
            if text_view.folded_heading_slugs() != source.folded_heading_slugs() {
                text_view.set_folded_heading_slugs(source.folded_heading_slugs());
            }
            if text_view.source_focus() != source.source_focus() {
                match source.source_focus() {
                    SourceFocus::None => text_view.clear_source_focus(),
                    SourceFocus::Document => text_view.focus_entire_source(),
                    SourceFocus::Scoped(range) => text_view.focus_source(range),
                }
            }
            if let Some(shared_selection) = &shared_selection {
                text_view.set_source_selected_ranges(shared_selection);
            }
            if let Some(offset) = shared_scroll_offset {
                text_view.scroll_to_offset(offset, ScrollPosition::Top, false);
            }
        }
        let mut state = self.markdown_document().state();
        state.zoom_level = source.zoom_level();
        state.folded_headings = source.folded_heading_slugs().into_iter().collect();
        self.markdown_document().set_state(state);
        // `defer { isSynchronizingPanes = false }`
        self.ivars().is_synchronizing_panes.set(false);
    }

    /// `setSharedZoom(_:)`.
    pub fn set_shared_zoom(&self, level: ZoomLevel) {
        let source = self.container_text_view();
        source.set_zoom_level(level);
        self.synchronize_panes(&source, false, false);
        let mut state = self.markdown_document().state();
        state.zoom_level = level;
        self.markdown_document().set_state(state);
        self.breadcrumb_view().set_zoom_level(level);
        self.announce_transient_status(Self::zoom_announcement(level));
    }

    fn zoom_announcement(level: ZoomLevel) -> &'static str {
        match level {
            ZoomLevel::H1 => "Top level — top-level headings",
            ZoomLevel::H2 => "Two levels — headings through level two",
            ZoomLevel::Headings => "Headings — all headings",
            ZoomLevel::Skeleton => "Skeleton — headings, first sentences, artifacts",
            ZoomLevel::Everything => "Everything — all content visible",
        }
    }

    /// `setSharedFolds(_:from:)`.
    pub fn set_shared_folds(&self, folded: HashSet<String>, source: Option<&MarkdownTextView>) {
        let source: Retained<MarkdownTextView> = match source {
            Some(source) => source.retain(),
            None => self.container_text_view(),
        };
        source.set_folded_heading_slugs(folded.clone());
        self.synchronize_panes(&source, false, false);
        let mut state = self.markdown_document().state();
        state.folded_headings = folded.into_iter().collect();
        self.markdown_document().set_state(state);
    }
}

use objc2::Message;

#[allow(dead_code)]
fn _unused(_: &NSNotification, _: NSRange) {}
