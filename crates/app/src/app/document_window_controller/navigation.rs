//! `// MARK: - Navigation` and `// MARK: - Split view (§9.3)` of
//! `DocumentWindowController.swift`, with focus mode's dimming overlays.

use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2_app_kit::{NSScrollView, NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect};
use upleft_core::NSRange;
use upleft_render::appkit_compat::RectExt;
use upleft_render::motion::{self, Curve};
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::{MarkdownTextViewDelegate, ScrollPosition};

use objc2::DefinedClass as _;
use super::{DocumentWindowController, FocusDimmingView};
use crate::app::themed_split_view::ThemedSplitView;
use crate::panels::appkit_support::{activate, downcast};
use crate::support::jump_history::Entry;

impl DocumentWindowController {
    // MARK: - Navigation

    /// `recordJump(to:label:)`.
    pub fn record_jump(&self, offset: isize, label: &str) {
        let url = self.markdown_document().url();
        let from = Entry::new(url.clone(), self.container_text_view().top_visible_offset(), "Reading position");
        self.jump_history().borrow_mut().record(Some(from), Entry::new(url, offset, label));
    }

    /// `jump(to:label:animated:)`.
    pub fn jump(&self, offset: isize, label: &str, animated: bool) {
        self.record_jump(offset, label);
        let source = self.container_text_view();
        // `.visible`: a jump to a target that is already on screen must not
        // move the page at all. Back/Forward keep `.center`: those restore a
        // *recorded* reading position, which is a deliberate reframe.
        source.scroll_to_offset(offset, ScrollPosition::Visible, animated);
        self.synchronize_panes(&source, false, false);
        self.refresh_breadcrumb();
    }

    /// `goBack()`.
    pub fn go_back(&self) {
        let Some(entry) = self.jump_history().borrow_mut().go_back() else { return };
        let source = self.container_text_view();
        source.scroll_to_offset(entry.offset, ScrollPosition::Center, true);
        self.synchronize_panes(&source, false, false);
    }

    /// `goForward()`.
    pub fn go_forward(&self) {
        let Some(entry) = self.jump_history().borrow_mut().go_forward() else { return };
        let source = self.container_text_view();
        source.scroll_to_offset(entry.offset, ScrollPosition::Center, true);
        self.synchronize_panes(&source, false, false);
    }

    // MARK: - Split view (§9.3)

    /// `toggleSplitView()`.
    pub fn toggle_split_view(&self) {
        let root = self.root_view();
        let primary = self.primary_container();
        if let Some(split) = self.split_view_container() {
            // Preserve the pane the user was actually working in. Removing a
            // focused split pane without handing its caret and camera back to
            // the primary leaves the window with no text first responder.
            let active = self.container_text_view();
            let active_top_offset = active.top_visible_offset();
            let active_viewport_y = active.enclosingScrollView().map(|scroll_view| scroll_view.contentView().bounds().origin.y);
            self.synchronize_panes(&active, true, true);
            let split_container = self.split_container();
            self.ivars().focus_dimming_views.borrow_mut().retain(|view| {
                let in_split = match (unsafe { view.superview() }, split_container.as_ref()) {
                    (Some(superview), Some(container)) => std::ptr::eq(&*superview, &***container as &NSView),
                    (None, None) => true,
                    _ => false,
                };
                if in_split {
                    view.removeFromSuperview();
                    return false;
                }
                true
            });
            primary.removeFromSuperview();
            split.removeFromSuperview();
            self.set_split_view_container(None);
            self.set_split_container(None);
            primary.setTranslatesAutoresizingMaskIntoConstraints(false);
            root.addSubview(&primary);
            activate(&[
                primary.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
                primary.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
                primary.topAnchor().constraintEqualToAnchor(&self.bar_stack().bottomAnchor()),
                primary.bottomAnchor().constraintEqualToAnchor(&self.status_bar_view().topAnchor()),
            ]);
            root.layoutSubtreeIfNeeded();
            primary.text_view().resize_to_fit_content();
            // Reflowing from half-width back to full-width changes physical Y
            // geometry. Restore by source anchor after that reflow.
            primary.text_view().scroll_to_offset(active_top_offset, ScrollPosition::Top, false);
            if let Some(active_viewport_y) = active_viewport_y
                && let Some(scroll_view) = primary.text_view().enclosingScrollView()
            {
                let clip = scroll_view.contentView();
                clip.scrollToPoint(NSPoint::new(clip.bounds().origin.x, active_viewport_y));
                scroll_view.reflectScrolledClipView(&clip);
            }
            let mut state = self.markdown_document().state();
            state.split_view_enabled = false;
            self.markdown_document().set_state(state);
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(primary.text_view()));
            }
            return;
        }

        // Two panes over the same buffer — the second container shares the
        // document's storage, so an edit in one appears in the other with no
        // synchronisation code at all (§3.1 paying off).
        let mtm = self.mtm();
        let second = MarkdownContainerView::with_storage(self.markdown_document().storage(), mtm);
        let delegate: Weak<dyn MarkdownTextViewDelegate> = Rc::downgrade(&self.delegates()) as _;
        second.text_view().set_markdown_delegate(Some(delegate));
        self.wire_key_event_handler(second.text_view());
        second.set_style_sheet(self.active_style_sheet());
        self.configure_local_asset_access(second.text_view(), self.markdown_document().url().as_ref());
        second.text_view().set_configuration(self.render_configuration());
        let source = self.container_text_view();
        let source_viewport_offset = source.top_visible_offset();
        second.text_view().set_mode(source.mode());
        second.text_view().set_zoom_level(source.zoom_level());
        second.text_view().set_folded_heading_slugs(source.folded_heading_slugs());
        second.text_view().adopt_shared_presentation(&source);
        second.text_view().set_source_selected_ranges(&source.source_selected_ranges());
        self.set_split_container(Some(second.clone()));

        let split = ThemedSplitView::new(self.active_style_sheet(), true, mtm);
        split.setTranslatesAutoresizingMaskIntoConstraints(false);

        primary.removeFromSuperview();
        primary.setTranslatesAutoresizingMaskIntoConstraints(true);
        second.setTranslatesAutoresizingMaskIntoConstraints(true);
        split.addArrangedSubview(&primary);
        split.addArrangedSubview(&second);
        root.addSubview(&split);
        activate(&[
            split.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
            split.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
            split.topAnchor().constraintEqualToAnchor(&self.bar_stack().bottomAnchor()),
            split.bottomAnchor().constraintEqualToAnchor(&self.status_bar_view().topAnchor()),
        ]);
        root.layoutSubtreeIfNeeded();
        split.setPosition_ofDividerAtIndex((split.bounds().width() / 2.0).max(1.0), 0);
        // A detached text view has no viewport geometry, so scrolling it
        // before `addArrangedSubview` silently lands at the document start.
        // Establish the final pane widths first, then restore the
        // source-space reading position the user was at.
        split.layoutSubtreeIfNeeded();
        second.text_view().resize_to_fit_content();
        second.text_view().scroll_to_offset(source_viewport_offset, ScrollPosition::Top, false);
        self.set_split_view_container(Some(split));
        let mut state = self.markdown_document().state();
        state.split_view_enabled = true;
        self.markdown_document().set_state(state);
        if self.is_focus_mode_enabled() {
            self.install_focus_dimming_view(&second);
        }
        self.synchronize_panes(&source, false, false);
    }

    pub(super) fn install_focus_dimming_view(&self, container: &MarkdownContainerView) {
        let already = self.ivars().focus_dimming_views.borrow().iter().any(|view| {
            unsafe { view.superview() }.is_some_and(|superview| std::ptr::eq(&*superview, container as &NSView))
        });
        if already {
            return;
        }
        let overlay = FocusDimmingView::new(self.mtm());
        overlay.setTranslatesAutoresizingMaskIntoConstraints(false);
        container.addSubview_positioned_relativeTo(&overlay, NSWindowOrderingMode::Above, None);
        activate(&[
            overlay.leadingAnchor().constraintEqualToAnchor(&container.leadingAnchor()),
            overlay.trailingAnchor().constraintEqualToAnchor(&container.trailingAnchor()),
            overlay.topAnchor().constraintEqualToAnchor(&container.topAnchor()),
            overlay.bottomAnchor().constraintEqualToAnchor(&container.bottomAnchor()),
        ]);
        let reduce_motion = self.active_style_sheet().reduce_motion;
        overlay.setAlphaValue(if reduce_motion { 1.0 } else { 0.0 });
        self.ivars().focus_dimming_views.borrow_mut().push(overlay.clone());
        motion::run(
            reduce_motion,
            motion::QUICK,
            Curve::Decelerate,
            move |_| {
                overlay.setAlphaValue(1.0);
            },
            None,
        );
    }

    pub(super) fn remove_focus_dimming_views(&self, animated: bool) {
        let views: Vec<Retained<FocusDimmingView>> = std::mem::take(&mut *self.ivars().focus_dimming_views.borrow_mut());
        if !(animated && !self.active_style_sheet().reduce_motion) {
            for view in &views {
                view.removeFromSuperview();
            }
            return;
        }
        let fading = views.clone();
        motion::run(
            false,
            motion::QUICK,
            Curve::Decelerate,
            move |_| {
                for view in &fading {
                    view.setAlphaValue(0.0);
                }
            },
            Some(Box::new(move || {
                for view in &views {
                    view.removeFromSuperview();
                }
            })),
        );
    }

    /// `updateFocusDimmingViews()`.
    pub fn update_focus_dimming_views(&self) {
        if !self.is_focus_mode_enabled() || self.ivars().focus_dimming_views.borrow().is_empty() {
            return;
        }
        let overlays = self.ivars().focus_dimming_views.borrow().clone();
        for overlay in &overlays {
            let Some(container) = (unsafe { overlay.superview() }) else { continue };
            let text_view = container
                .subviews()
                .iter()
                .find_map(|view| downcast::<NSScrollView>(&view))
                .and_then(|scroll_view| scroll_view.documentView())
                .and_then(|document| downcast::<MarkdownTextView>(&document));
            let Some(text_view) = text_view else { continue };
            overlay.set_highlight_rect(self.focus_rect(&text_view, &container));
        }
        for overlay in &overlays {
            overlay.setNeedsDisplay(true);
        }
    }

    fn focus_rect(&self, text_view: &MarkdownTextView, container: &NSView) -> Option<NSRect> {
        let storage = self.markdown_document().storage().string();
        let length = storage.length() as isize;
        if !(length > 0) {
            return None;
        }
        let offset = 0.max(text_view.source_selected_range().location).min(length - 1);
        let paragraph =
            storage.paragraphRangeForRange(objc2_foundation::NSRange::new(offset as usize, 0));
        let paragraph = NSRange { location: paragraph.location as isize, length: paragraph.length as isize };
        let start = text_view.rect_for_offset(paragraph.location)?;
        let end = text_view
            .rect_for_offset(paragraph.location.max(paragraph.location + paragraph.length - 1))
            .unwrap_or(start);
        Some(text_view.convertRect_toView(start.union(end).inset_by(-8.0, -4.0), Some(container)))
    }
}
