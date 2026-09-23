//! `// MARK: - Opening` of `DocumentWindowController.swift`:
//! `open(_:mode:)`, the first-frame restore, `resetTransientChrome`,
//! `dumpLayoutIfRequested` (`DOWNRIGHT_DEBUG_LAYOUT` / `_CAPTURE` / `_FIND`
//! / `_PANELS`) and `adopt(text:title:)`.

use std::io::Write as _;

use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSBitmapImageFileType, NSView};
use objc2_core_foundation::{CGAffineTransform, CGRect, CGSize};
use objc2_foundation::{NSDictionary, NSEdgeInsets, NSString};
use upleft_core::{DirtySet, NSRange};
use upleft_foundation::json_encoder::double_description;
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::main_async;
use upleft_render::motion::{self, Curve};
use upleft_render::render_contracts::RenderMode;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use objc2::DefinedClass as _;
use objc2_app_kit::NSAnimatablePropertyContainer as _;
use super::DocumentWindowController;
use crate::ai::document_state_store::DocumentStateStore;
use crate::ai::markdown_document::{DocumentError, Unavailable, UnreadChanges};
use crate::ai::path_resolver::PathResolver;
use crate::ai::sibling_scanner::SiblingScanner;
use crate::panels::appkit_support::IDENTITY;
use crate::support::preferences::Preferences;

impl DocumentWindowController {
    // MARK: - Opening

    /// `open(_:mode:) throws`.
    pub fn open(&self, url: &FileUrl, mode: RenderMode) -> Result<(), DocumentError> {
        self.set_is_opening_document(true);
        let result = self.open_body(url, mode);
        // `defer { isOpeningDocument = false }`
        self.set_is_opening_document(false);
        result
    }

    fn open_body(&self, url: &FileUrl, mode: RenderMode) -> Result<(), DocumentError> {
        self.reset_transient_chrome();
        // The suppression belongs to the buffer the user declined to keep; a
        // different document opened in this window starts with a clean slate.
        self.ivars().implicit_save_suppressed.set(false);
        self.markdown_document().open(url)?;
        self.clear_stale_full_document_selection_if_needed();
        let requested_mode = mode.normalized_for_editing();
        self.set_mode(requested_mode);

        self.set_scanner(Some(std::rc::Rc::new(SiblingScanner::new(
            url,
            Preferences::shared().values().sibling_scan_directories,
        ))));
        self.set_path_resolver(Some(PathResolver::new(Some(url))));
        self.configure_local_asset_access(self.primary_container().text_view(), Some(url));

        if let Some(window) = self.window() {
            window.setTitle(&NSString::from_str(&url.last_path_component()));
        }
        if let Some(window) = self.window() {
            window.setSubtitle(&NSString::from_str(&url.deleting_last_path_component().last_path_component()));
        }
        if let Some(window) = self.window() {
            window.setRepresentedURL(Some(&url.to_nsurl()));
        }
        self.apply_mode(requested_mode);
        self.apply_render_configuration();
        let state = self.markdown_document().state();
        let primary = self.primary_container();
        primary.text_view().set_zoom_level(state.zoom_level);
        primary.text_view().set_folded_heading_slugs(state.folded_headings.iter().cloned().collect());
        // Structure-only tree is ready; full decoration follows via onReparse.
        primary.text_view().update(self.markdown_document().parsed(), &DirtySet::wholesale(), false);
        // AppKit keeps the old text view's native selection while the shared
        // storage is replaced. That selection belongs to the previous file,
        // not to this open operation; the deferred restore below will apply
        // this document's persisted caret/selection after the first frame.
        primary.text_view().set_source_selected_ranges(&[NSRange { location: 0, length: 0 }]);

        primary.setWantsLayer(true);
        let reduce_motion = self.active_style_sheet().reduce_motion;
        primary.setAlphaValue(if reduce_motion { 1.0 } else { 0.0 });
        if let Some(layer) = primary.layer() {
            layer.setAffineTransform(CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: -6.0 });
        }
        let animated = primary.clone();
        motion::run(
            reduce_motion,
            motion::QUICK,
            Curve::Decelerate,
            move |_| {
                let animator = animated.animator();
                animator.setAlphaValue(1.0);
                if let Some(layer) = animated.layer() {
                    layer.setAffineTransform(IDENTITY);
                }
            },
            None,
        );

        self.schedule_derived_ui_refresh(true);
        if self.markdown_document().state().split_view_enabled {
            self.toggle_split_view();
        }
        self.apply_focus_mode(Preferences::shared().values().focus_mode, false);

        // Paint a deterministic first frame. Apply a saved deep offset only
        // after the document surface has had a chance to establish its TextKit
        // viewport; restoring it during the first layout pass can otherwise
        // produce a blank surface until the first user scroll.
        let generation = self.ivars().initial_restore_generation.get().wrapping_add(1);
        self.ivars().initial_restore_generation.set(generation);
        let restore_generation = generation;
        self.ivars()
            .initial_restore_viewport_y
            .set(Some(self.primary_container().scroll_view().contentView().bounds().origin.y));
        self.ivars().pending_initial_restore_offset.set(Some(0));
        self.ivars().deferred_initial_restore_offset.set(Some(self.markdown_document().restored_offset()));
        let weak = objc2::rc::Weak::new(self);
        main_async(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().initial_restore_generation.get() != restore_generation {
                return;
            }
            this.restore_initial_reading_position_if_ready();
        });
        Ok(())
    }

    /// A pre-fix build could persist the synthetic full-range selection that
    /// AppKit created while replacing a document's shared storage. Full-source
    /// selections are transient commands, not a useful reopen position, so
    /// migrate that stale state to a caret before the first frame is painted.
    fn clear_stale_full_document_selection_if_needed(&self) {
        let document = self.markdown_document();
        let length = document.storage().length() as isize;
        let state = document.state();
        if !(length > 0 && state.selection_location <= 0 && state.selection_length >= length) {
            return;
        }
        let Some(document_url) = document.url() else { return };
        let mut state = state;
        state.selection_location = 0;
        state.selection_length = 0;
        document.set_state(state.clone());
        DocumentStateStore::shared().save(&state, &document_url);
    }

    /// Clears find / conflict / change chrome that must not survive a
    /// document hop.
    pub fn reset_transient_chrome(&self) {
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
        self.ivars().cached_metrics_document_id.set(None);
        *self.ivars().cached_section_metrics.borrow_mut() = Vec::new();
        self.ivars().cached_word_count.set(0);

        self.stop_speaking();
        self.find_session().borrow_mut().clear();
        for pane in self.document_panes() {
            pane.text_view().set_search_hits(Vec::new());
            pane.text_view().set_current_search_hit(None);
        }
        if let Some(bar) = self.find_bar() {
            bar.set_status_text("");
        }
        self.dismiss_conflict_bar();
        self.dismiss_change_summary();
    }

    pub(super) fn restore_initial_reading_position_if_ready(&self) {
        let Some(restored) = self.ivars().pending_initial_restore_offset.get() else { return };
        if self.window().map(|window| window.isVisible()) != Some(true) {
            return;
        }
        let Some(primary) = self.primary_container_opt() else { return };
        let clip = primary.scroll_view().contentView();
        if let Some(expected) = self.ivars().initial_restore_viewport_y.get()
            && (clip.bounds().origin.y - expected).abs() > 0.5
        {
            // A real scroll/edit happened before the deferred first frame.
            // Do not put the saved camera back under the user's hands.
            self.ivars().pending_initial_restore_offset.set(None);
            self.ivars().deferred_initial_restore_offset.set(None);
            self.ivars().initial_restore_viewport_y.set(None);
            return;
        }
        self.ivars().pending_initial_restore_offset.set(None);

        if let Some(window) = self.window() {
            window.layoutIfNeeded();
        }
        self.root_view().layoutSubtreeIfNeeded();
        primary.layoutSubtreeIfNeeded();
        primary.text_view().resize_to_fit_content();

        // TextKit 2 can defer the first rendering surface when the initial
        // bounds jump straight into a deep, restored section. Prime the
        // document once at the top before applying the saved position. This
        // stays off-screen, but makes the first visible frame deterministic.
        primary.text_view().scroll_to_offset(0, ScrollPosition::Top, false);
        primary.text_view().prepare_for_display();
        primary.text_view().displayIfNeeded();
        primary.text_view().scroll_to_offset(restored, ScrollPosition::Top, false);
        primary.text_view().prepare_for_display();
        primary.text_view().setNeedsDisplay(true);
        primary.scroll_view().contentView().setNeedsDisplay(true);
        self.ivars().initial_restore_viewport_y.set(Some(clip.bounds().origin.y));
        self.update_breadcrumb_and_gutter();

        let document = self.markdown_document();
        let state = document.state();
        let storage_length = document.storage().length() as isize;
        let selection = NSRange { location: state.selection_location.min(storage_length), length: 0 };
        let available = storage_length - selection.location;
        primary
            .text_view()
            .set_source_selected_ranges(&[NSRange { location: selection.location, length: state.selection_length.min(available) }]);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(primary.text_view()));
        }
        match document.unread_changes() {
            UnreadChanges::None => {}
            UnreadChanges::Marked { .. } => self.present_unread_changes(),
            UnreadChanges::PreviousVersionUnavailable { reason } => {
                // Still say the file moved on, even when the bytes to diff
                // against are gone — silence would read as "nothing happened".
                self.show_change_summary(Some(if reason == Unavailable::Corrupt {
                    "Changed since you last read it — the previous version is damaged"
                } else {
                    "Changed since you last read it — the previous version is no longer available"
                }));
            }
        }
        self.dump_layout_if_requested();

        let expected_after_first_restore =
            self.ivars().initial_restore_viewport_y.get().unwrap_or_else(|| clip.bounds().origin.y);
        let restore_generation = self.ivars().initial_restore_generation.get();
        let weak = objc2::rc::Weak::new(self);
        let first_clip = clip.clone();
        main_async(move || {
            let Some(this) = weak.load() else { return };
            if !(this.ivars().initial_restore_generation.get() == restore_generation
                && (first_clip.bounds().origin.y - expected_after_first_restore).abs() <= 0.5)
            {
                return;
            }
            let primary = this.primary_container();
            primary.text_view().scroll_to_offset(restored, ScrollPosition::Top, false);
            primary.text_view().prepare_for_display();
            primary.text_view().displayIfNeeded();
            primary.scroll_view().contentView().displayIfNeeded();
            this.ivars().initial_restore_viewport_y.set(Some(first_clip.bounds().origin.y));
        });

        let Some(deferred) = self.ivars().deferred_initial_restore_offset.get() else { return };
        if !(deferred > 0) {
            return;
        }
        self.ivars().deferred_initial_restore_offset.set(None);
        let expected_before_deferred_restore =
            self.ivars().initial_restore_viewport_y.get().unwrap_or_else(|| clip.bounds().origin.y);
        let weak = objc2::rc::Weak::new(self);
        main_async(move || {
            let Some(this) = weak.load() else { return };
            if !(this.ivars().initial_restore_generation.get() == restore_generation
                && this.window().map(|window| window.isVisible()) == Some(true)
                && (clip.bounds().origin.y - expected_before_deferred_restore).abs() <= 0.5)
            {
                return;
            }
            if let Some(window) = this.window() {
                window.layoutIfNeeded();
            }
            let primary = this.primary_container();
            primary.layoutSubtreeIfNeeded();
            primary.text_view().scroll_to_offset(deferred, ScrollPosition::Top, false);
            primary.text_view().prepare_for_display();
            primary.text_view().displayIfNeeded();
            primary.scroll_view().contentView().displayIfNeeded();
            this.ivars().initial_restore_viewport_y.set(Some(clip.bounds().origin.y));
            this.update_breadcrumb_and_gutter();
        });
    }

    /// `DOWNRIGHT_DEBUG_LAYOUT=1` dumps the view geometry once the window has
    /// laid out. A blank document area has exactly one cause — some view in
    /// this chain has no height — and guessing which is slower than printing.
    pub fn dump_layout_if_requested(&self) {
        if std::env::var_os("DOWNRIGHT_DEBUG_LAYOUT").is_none() {
            return;
        }
        let mtm = self.mtm();
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        if let Some(window) = self.window() {
            window.makeKeyAndOrderFront(None);
        }
        if std::env::var_os("DOWNRIGHT_DEBUG_FIND").is_some() && self.find_bar().is_none() {
            self.show_find_bar(false, None);
            if let Some(bar) = self.find_bar() {
                bar.setAlphaValue(1.0);
            }
            if let Some(layer) = self.find_bar().and_then(|bar| bar.layer()) {
                layer.setAffineTransform(IDENTITY);
            }
        }
        if std::env::var_os("DOWNRIGHT_DEBUG_PANELS").is_some() && self.task_panel().is_none() {
            self.toggle_task_panel();
        }
        if let Some(window) = self.window() {
            window.layoutIfNeeded();
        }
        let root = self.root_view();
        root.layoutSubtreeIfNeeded();
        let window = self.window();
        let primary = self.primary_container();
        let text_view = primary.text_view();
        let scroll_view = primary.scroll_view();
        let zero = CGRect::default();
        let style_sheet = text_view.style_sheet();
        let lines = [
            format!("window       {}", describe_rect(window.as_ref().map(|window| window.frame()).unwrap_or(zero))),
            format!(
                "contentView  {}",
                describe_rect(window.as_ref().and_then(|window| window.contentView()).map(|view| view.frame()).unwrap_or(zero))
            ),
            format!("rootView     {}", describe_rect(root.frame())),
            format!(
                "barStack     {}  arranged={}",
                describe_rect(self.bar_stack().frame()),
                self.bar_stack().arrangedSubviews().count()
            ),
            format!("container    {}", describe_rect(primary.frame())),
            format!("  scrollView {}", describe_rect(scroll_view.frame())),
            format!("  clipView   {}", describe_rect(scroll_view.contentView().frame())),
            format!("  docView    {}", describe_rect(scroll_view.documentView().map(|view| view.frame()).unwrap_or(zero))),
            format!("  textView   {}", describe_rect(text_view.frame())),
            format!(
                "  container  {}",
                describe_size(unsafe { text_view.textContainer() }.map(|container| container.size()).unwrap_or_default())
            ),
            format!("  insets     {}", describe_insets(scroll_view.contentInsets())),
            format!(
                "breadcrumb   {}  fitting={}",
                describe_rect(self.breadcrumb_view().frame()),
                describe_size(self.breadcrumb_view().fittingSize())
            ),
            format!(
                "gutter       {}  fitting={}",
                describe_rect(self.density_gutter_view().frame()),
                describe_size(self.density_gutter_view().fittingSize())
            ),
            format!("storage      {} chars", self.markdown_document().storage().length()),
            format!("clipBounds   {}", describe_rect(scroll_view.contentView().bounds())),
            format!("tvInContainer {}", describe_rect(text_view.convertRect_toView(text_view.bounds(), Some(&primary)))),
            format!(
                "scrollBG     {} drawsBG={}",
                describe_object(&scroll_view.backgroundColor()),
                scroll_view.drawsBackground()
            ),
            format!(
                "tvBG         {} / {}",
                describe_object(&style_sheet.background),
                describe_object(&text_view.backgroundColor())
            ),
            format!("tvDrawsBG    {}", text_view.drawsBackground()),
            format!("measureWidth {}", double_description(style_sheet.measure_width)),
        ];
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(format!("\n--- Upleft layout ---\n{}\n", lines.join("\n")).as_bytes());

        if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
            for item in toolbar.items() {
                let view = item.view();
                let _ = stderr.write_all(
                    format!(
                        "toolbar {} view={} frame={} intrinsic={} hidden={}\n",
                        item.itemIdentifier(),
                        "Optional<NSView>",
                        describe_rect(view.as_ref().map(|view| view.frame()).unwrap_or(zero)),
                        describe_size(view.as_ref().map(|view| view.intrinsicContentSize()).unwrap_or_default()),
                        view.as_ref().map(|view| view.isHidden()).unwrap_or(false)
                    )
                    .as_bytes(),
                );
            }
        }

        // The inspector is the other place a "blank rectangle" can come from:
        // its header lives above the panel, so a header laid out behind the
        // toolbar or at zero height reads as dead space rather than as a bug.
        if let Some(host) = self.inspector_host() {
            let selected = match host.selected_section() {
                Some(section) => format!("Optional(DownrightApp.InspectorSection.{})", section_case_name(section)),
                None => "nil".to_owned(),
            };
            let mut lines = vec![format!(
                "inspector    host={} safeArea={} selected={}",
                describe_rect(host.frame()),
                describe_insets(host.safeAreaInsets()),
                selected
            )];
            for sub in host.subviews() {
                lines.push(format!(
                    "  {} frame={} hidden={} alpha={}",
                    class_name(&sub),
                    describe_rect(sub.frame()),
                    sub.isHidden(),
                    double_description(sub.alphaValue())
                ));
            }
            let _ = stderr.write_all(format!("{}\n", lines.join("\n")).as_bytes());
        }

        let Some(directory) = std::env::var("DOWNRIGHT_DEBUG_CAPTURE").ok() else { return };

        // The titlebar, traffic lights and toolbar are drawn by the window's
        // frame view, not by our content view, so a capture of the content
        // alone shows the document without any of the chrome around it.
        if let Some(frame_view) = self.window().and_then(|window| window.contentView()).and_then(|view| unsafe { view.superview() })
            && frame_view.bounds().size.width > 0.0
            && frame_view.bounds().size.height > 0.0
            && let Some(rep) = frame_view.bitmapImageRepForCachingDisplayInRect(frame_view.bounds())
        {
            frame_view.cacheDisplayInRect_toBitmapImageRep(frame_view.bounds(), &rep);
            if let Some(png) = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) } {
                let path = FileUrl::from_path(&directory).appending_path_component("window.png").path();
                let _ = png.writeToFile_atomically(&NSString::from_str(&path), false);
            }
        }

        let views: [(&str, Retained<NSView>); 5] = [
            ("root", root.clone()),
            ("container", Retained::into_super(primary.clone())),
            ("scroll", Retained::into_super(scroll_view.clone())),
            ("clip", Retained::into_super(scroll_view.contentView())),
            ("textview", Retained::into_super(Retained::into_super(Retained::into_super(text_view.clone())))),
        ];
        for (name, view) in views {
            if !(view.bounds().size.width > 0.0 && view.bounds().size.height > 0.0) {
                continue;
            }
            let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(view.bounds()) else { continue };
            view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &rep);
            let Some(png) = (unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }) else {
                continue;
            };
            let path = FileUrl::from_path(&directory).appending_path_component(&format!("{name}.png")).path();
            let _ = png.writeToFile_atomically(&NSString::from_str(&path), false);
        }
        let _ = stderr.write_all(format!("captured to {directory}\n").as_bytes());
    }

    /// `adopt(text:title:)`.
    pub fn adopt(&self, text: &str, title: &str) {
        self.markdown_document().adopt(text, None);
        if let Some(window) = self.window() {
            window.setTitle(&NSString::from_str(title));
        }
        self.apply_mode(RenderMode::Live);
        self.primary_container().text_view().update(self.markdown_document().parsed(), &DirtySet::wholesale(), true);
        self.refresh_derived_ui();
    }
}

// MARK: - Swift `description`s for the layout dump

/// `"\(rect)"`: `(x, y, width, height)`.
fn describe_rect(rect: CGRect) -> String {
    format!(
        "({}, {}, {}, {})",
        double_description(rect.origin.x),
        double_description(rect.origin.y),
        double_description(rect.size.width),
        double_description(rect.size.height)
    )
}

/// `"\(size)"`: `(width, height)`.
fn describe_size(size: CGSize) -> String {
    format!("({}, {})", double_description(size.width), double_description(size.height))
}

/// `"\(insets)"`: `NSEdgeInsets(top: …, left: …, bottom: …, right: …)`.
fn describe_insets(insets: NSEdgeInsets) -> String {
    format!(
        "NSEdgeInsets(top: {}, left: {}, bottom: {}, right: {})",
        double_description(insets.top),
        double_description(insets.left),
        double_description(insets.bottom),
        double_description(insets.right)
    )
}

/// `type(of: view)`: the class name (Swift classes are registered under
/// their unqualified Swift names).
fn class_name(view: &NSView) -> String {
    view.class().name().to_string_lossy().into_owned()
}

/// The case name `String(describing:)` prints for an `InspectorSection`.
fn section_case_name(section: crate::panels::inspector_host_view::InspectorSection) -> &'static str {
    use crate::panels::inspector_host_view::InspectorSection;
    match section {
        InspectorSection::Tasks => "tasks",
        InspectorSection::History => "history",
        InspectorSection::Context => "context",
        InspectorSection::Search => "search",
    }
}

/// `"\(color)"`: the object's `description`.
fn describe_object(object: &objc2_app_kit::NSColor) -> String {
    let description: Retained<NSString> = unsafe { objc2::msg_send![object, description] };
    description.to_string()
}
