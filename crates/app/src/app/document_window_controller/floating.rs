//! `// MARK: - Panels` of `DocumentWindowController.swift`: the floating
//! inspector surface (presentation, morph anchors, refits, dismissal), the
//! Tasks panel, the inspector host, and the auxiliary windows the controller
//! keeps alive.

use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::Weak as ObjcWeak;
use objc2::sel;
use objc2_app_kit::{
    NSAnimationContext, NSApplication, NSApplicationDidBecomeActiveNotification, NSAutoresizingMaskOptions, NSView,
    NSWindow, NSWindowController, NSWindowOrderingMode, NSWindowWillCloseNotification,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue, NSRect};
use upleft_render::appkit_compat::{RECT_ZERO, RectExt, main_after, main_async, rect};
use upleft_render::motion::{self, Curve, MorphAnchor};

use objc2::DefinedClass as _;
use objc2::MainThreadOnly as _;
use objc2_app_kit::{NSAnimatablePropertyContainer as _, NSAppearanceCustomization as _};
use upleft_render::appkit_compat::RectExt as _;
use super::DocumentWindowController;
use crate::app::document_window::DocumentWindow;
use crate::panels::appkit_support::{activate, downcast};
use crate::panels::chrome_glass::ChromeGlass;
use crate::panels::floating_panel_surface::{FloatingPanelSurface, FloatingPanelWindow, Top};
use crate::panels::inspector_host_view::{InspectorHostView, InspectorSection};
use crate::panels::panel_chrome::PanelMetrics;
use crate::panels::task_panel_view::{TaskPanelDelegate, TaskPanelView};
use crate::panels::task_progress_ring::TaskProgressRing;

impl DocumentWindowController {
    // MARK: - Panels

    /// The Tasks panel morphs from the toolbar ring and hangs over the
    /// document: travelling glass is aimed at a measured frame instead of a
    /// docked pane.
    ///
    /// Dismissal rule for the floating surface — one panel, one way out:
    /// Esc, the header close button, a second press of the ring, or a click
    /// on the document outside the glass. Ordinary app switching is
    /// deliberately not a dismissal.
    fn present_floating_surface(&self, surface: &FloatingPanelSurface) {
        let mtm = self.mtm();
        let Some(window) = self.window() else { return };
        let Some(target) = window.contentView() else { return };
        let morphs_from_control = self.uses_floating_control_morph();
        let width =
            surface.preferred_width().min((target.bounds().width() - 2.0 * PanelMetrics::FLOATING_MARGIN).max(200.0));
        let cap = self.floating_height_cap(&target);
        let frame = self.floating_frame(surface, &target, width, cap);
        let resting = Self::screen_frame(frame, &target, &window);
        let sliver = rect(
            resting.min_x(),
            resting.max_y() - Top::POUR_SLIVER_HEIGHT,
            resting.width(),
            Top::POUR_SLIVER_HEIGHT,
        );

        // Keep native glass in a transparent child boundary. This gives
        // AppKit a real compositor surface to sample against the document;
        // placing the effect inside the document glass group flattens it.
        let shadow_margin = PanelMetrics::FLOATING_SHADOW_MARGIN;
        let child_frame = resting.inset_by(-shadow_margin, -shadow_margin);
        let child = FloatingPanelWindow::new(child_frame, mtm);
        child.setAlphaValue(if morphs_from_control { 0.0 } else { 1.0 });
        child.setAppearance(ChromeGlass::material_appearance(&self.active_style_sheet()).as_deref());
        child.set_floating_surface(Some(surface));
        let weak = ObjcWeak::new(self);
        child.set_on_outside_mouse_down(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.restore_floating_focus_and_close();
            }
        })));
        let child_content = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, child_frame.width(), child_frame.height()));
        child_content.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        child_content.setClipsToBounds(false);
        child.setContentView(Some(&child_content));
        surface.setFrame(rect(shadow_margin, shadow_margin, resting.width(), resting.height()));
        surface.configure_window_frames(resting, sliver, frame.height());
        let weak_child = ObjcWeak::new(&*child);
        let weak_surface = ObjcWeak::new(surface);
        surface.set_on_window_frame_change(Some(Rc::new(move |frame: NSRect| {
            let (Some(child), Some(surface)) = (weak_child.load(), weak_surface.load()) else { return };
            if !(frame.origin.x.is_finite()
                && frame.origin.y.is_finite()
                && frame.size.width.is_finite()
                && frame.size.height.is_finite()
                && frame.size.width > 1.0
                && frame.size.height > 1.0)
            {
                return;
            }
            // Never feed an invalid frame into either the child window or the
            // surface's local shadow inset during activation or resize.
            child.setFrame_display(frame.inset_by(-shadow_margin, -shadow_margin), true);
            surface.setFrame(rect(shadow_margin, shadow_margin, frame.size.width, frame.size.height));
            surface.setNeedsLayout(true);
        })));
        let weak = ObjcWeak::new(self);
        let weak_surface = ObjcWeak::new(surface);
        surface.set_on_frame_spring_settled(Some(Rc::new(move || {
            let (Some(this), Some(surface)) = (weak.load(), weak_surface.load()) else { return };
            if !this.floating_surface().is_some_and(|current| std::ptr::eq(&*current, &*surface)) {
                return;
            }
            if surface.is_dismissing() {
                this.remove_floating_surface();
            } else if this.floating_presentation_needs_focus() {
                this.set_floating_presentation_needs_focus(false);
                this.focus_floating_surface(&surface);
                this.refresh_toolbar_selection_state();
            }
        })));
        let source_anchor = self.floating_source_anchor(&window);
        self.set_floating_control_anchor_frame(source_anchor.frame);
        let source_frame = window.convertRectToScreen(source_anchor.frame);
        if morphs_from_control {
            self.progress_ring().setAlphaValue(0.0);
            surface.prepare_anchor_presentation(source_frame);
        }
        child_content.addSubview(surface);
        surface.layoutSubtreeIfNeeded();
        unsafe { window.addChildWindow_ordered(&child, NSWindowOrderingMode::Above) };
        child.orderFront(None);
        surface.refresh_glass_after_window_attach();

        if !morphs_from_control {
            surface.set_resting_frame(resting);
        }
        surface.layoutSubtreeIfNeeded();
        self.set_floating_surface(Some(surface.retain()));
        self.set_floating_panel_window(Some(child.clone()));
        self.set_floating_presentation_needs_focus(morphs_from_control);
        if let Some(document_window) = downcast::<DocumentWindow>(&window) {
            document_window.set_floating_surface(Some(surface));
            let weak = ObjcWeak::new(self);
            document_window.set_on_floating_outside_mouse_down(Some(Rc::new(move || {
                if let Some(this) = weak.load() {
                    this.restore_floating_focus_and_close();
                }
            })));
            let weak = ObjcWeak::new(self);
            document_window.set_on_floating_cancel(Some(Rc::new(move || {
                if let Some(this) = weak.load() {
                    this.close_inspector(true);
                }
            })));
        }
        let weak = ObjcWeak::new(self);
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            let weak = weak.clone();
            main_async(move || {
                if let Some(this) = weak.load() {
                    this.restore_floating_panel_window();
                }
            });
        });
        // SAFETY: the block runs on the main queue; the observer is removed
        // in `removeFloatingSurface`.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidBecomeActiveNotification),
                Some(&NSApplication::sharedApplication(mtm)),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *self.ivars().floating_activation_observer.borrow_mut() = Some(observer);
        self.set_floating_surface_frame(resting);
        if morphs_from_control {
            let weak = ObjcWeak::new(self);
            let weak_child = ObjcWeak::new(&*child);
            let weak_surface = ObjcWeak::new(surface);
            main_async(move || {
                let (Some(this), Some(child), Some(surface)) = (weak.load(), weak_child.load(), weak_surface.load())
                else {
                    return;
                };
                if !this.floating_surface().is_some_and(|current| std::ptr::eq(&*current, &*surface)) {
                    return;
                }
                child.setAlphaValue(1.0);
                surface.start_anchor_presentation(true);
                let weak = ObjcWeak::new(&*this);
                let weak_surface = ObjcWeak::new(&*surface);
                main_after(motion::DELIBERATE * 0.58, move || {
                    let (Some(this), Some(surface)) = (weak.load(), weak_surface.load()) else { return };
                    if !this.floating_surface().is_some_and(|current| std::ptr::eq(&*current, &*surface)) {
                        return;
                    }
                    this.reveal_progress_ring();
                    surface.play_morph_arrival_details();
                });
            });
        } else {
            surface.present_from_sliver(!self.active_style_sheet().reduce_motion);
            if !self.active_style_sheet().reduce_motion {
                let weak = ObjcWeak::new(self);
                let weak_surface = ObjcWeak::new(surface);
                main_after(motion::FLOATING_CONTENT_REVEAL_LEAD, move || {
                    let (Some(this), Some(surface)) = (weak.load(), weak_surface.load()) else { return };
                    if !this.floating_surface().is_some_and(|current| std::ptr::eq(&*current, &*surface)) {
                        return;
                    }
                    surface.play_morph_arrival_details();
                });
            }
            self.focus_floating_surface(surface);
        }
        self.refresh_toolbar_selection_state();
    }

    /// The same surface retreats into the toolbar ring. Offscreen and Reduce
    /// Motion dismissals use the deterministic sliver path.
    fn dismiss_floating_surface(&self) {
        let Some(surface) = self.floating_surface() else {
            self.remove_floating_surface();
            return;
        };
        let anchor = self.floating_control_anchor_frame();
        if self.uses_floating_control_morph()
            && let Some(window) = self.window()
            && anchor.width() > 1.0
            && anchor.height() > 1.0
        {
            let source = window.convertRectToScreen(anchor);
            surface.dismiss_to_anchor(source, true);
            return;
        }
        surface.dismiss_to_sliver(!self.active_style_sheet().reduce_motion);
    }

    /// Floating panels own their material transition.
    fn uses_floating_control_morph(&self) -> bool {
        let Some(window) = self.window() else { return false };
        !self.active_style_sheet().reduce_motion && window.isVisible() && window.screen().is_some()
    }

    /// The ring lends its substance to the outbound flight; as the card's
    /// material becomes legible it takes that substance back.
    fn reveal_progress_ring(&self) {
        if self.active_style_sheet().reduce_motion {
            self.progress_ring().setAlphaValue(1.0);
            return;
        }
        let ring = self.progress_ring().clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            let context = unsafe { context.as_ref() };
            context.setDuration(motion::QUICK);
            context.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
            let animator = unsafe { ring.animator() };
            animator.setAlphaValue(1.0);
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }

    pub(super) fn remove_floating_surface(&self) {
        if let Some(document_window) = self.window().and_then(|window| downcast::<DocumentWindow>(&window)) {
            document_window.set_floating_surface(None);
            document_window.set_on_floating_outside_mouse_down(None);
            document_window.set_on_floating_cancel(None);
        }
        let observer = self.ivars().floating_activation_observer.borrow_mut().take();
        if let Some(observer) = observer {
            // SAFETY: the token came from `addObserverForName:…`.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
        if let Some(child) = self.floating_panel_window() {
            child.orderOut(None);
        }
        if let (Some(parent), Some(child)) = (self.window(), self.floating_panel_window()) {
            parent.removeChildWindow(&child);
        }
        if let Some(surface) = self.floating_surface() {
            surface.set_on_window_frame_change(None);
        }
        if let Some(surface) = self.floating_surface() {
            surface.set_on_frame_spring_settled(None);
        }
        if let Some(surface) = self.floating_surface() {
            surface.removeFromSuperview();
        }
        let viewport_repairs = std::mem::take(&mut *self.ivars().floating_dismiss_viewport_repairs.borrow_mut());
        self.set_floating_panel_window(None);
        self.set_floating_surface(None);
        self.set_floating_presentation_needs_focus(false);
        self.set_floating_control_anchor_frame(RECT_ZERO);
        for repair in &viewport_repairs {
            repair();
        }
    }

    /// The morph's resting place in window space — the surface's own frame,
    /// or the frame it last rested at once the dismissal hands it off.
    fn floating_destination_anchor(&self, window: &NSWindow) -> NSRect {
        if let Some(surface) = self.floating_surface()
            && let Some(target) = window.contentView()
        {
            if surface.window().is_some() && surface.resting_window_frame_for_morph() != RECT_ZERO {
                let frame = window.convertRectFromScreen(surface.resting_window_frame_for_morph());
                self.set_floating_surface_frame(surface.resting_window_frame_for_morph());
                return frame;
            }
            let width = surface
                .preferred_width()
                .min((target.bounds().width() - 2.0 * PanelMetrics::FLOATING_MARGIN).max(200.0));
            let frame = self.floating_frame(&surface, &target, width, self.floating_height_cap(&target));
            self.set_floating_surface_frame(Self::screen_frame(frame, &target, window));
            return target.convertRect_toView(frame, None);
        }
        window.convertRectFromScreen(self.floating_surface_frame())
    }

    /// The ring-sized source anchor, exactly where the toolbar ring sits.
    fn floating_source_anchor(&self, window: &NSWindow) -> MorphAnchor {
        let destination = self.floating_destination_anchor(window);
        let side = TaskProgressRing::MORPH_SIDE;
        let ring = self.progress_ring();
        let tint = Some(self.active_style_sheet().accent.colorWithAlphaComponent(0.10));
        if ring.window().is_some_and(|ring_window| std::ptr::eq(&*ring_window, window)) {
            let frame = ring.convertRect_toView(ring.bounds(), None);
            return MorphAnchor {
                frame: rect(frame.mid_x() - side / 2.0, frame.mid_y() - side / 2.0, side, side),
                corner_radius: side / 2.0,
                tint,
            };
        }
        MorphAnchor {
            frame: rect(destination.max_x() - side, destination.max_y() - side, side, side),
            corner_radius: side / 2.0,
            tint,
        }
    }

    /// The cap lives on the window, so a resize moves the ceiling and the
    /// body re-clamps.
    fn floating_height_cap(&self, target: &NSView) -> CGFloat {
        let available_height = (target.bounds().height() - target.safeAreaInsets().top).max(0.0);
        ((Top::WINDOW_HEIGHT_FRACTION * available_height).min(available_height) - 2.0 * PanelMetrics::FLOATING_MARGIN)
            .max(40.0)
    }

    /// One frame formula for presentation, content refits, and window resize.
    /// `y` is derived from the target's flippedness once, so every path keeps
    /// the top edge under the toolbar and the trailing edge flush.
    pub fn floating_frame(&self, surface: &FloatingPanelSurface, target: &NSView, width: CGFloat, cap: CGFloat) -> NSRect {
        surface.prepare_for_measurement(width, cap);
        let fitted = surface.fitted_content_height();
        let desired = fitted.max(Top::MINIMUM_CONTENT_HEIGHT);
        let height = cap.min(desired);
        let margin = PanelMetrics::FLOATING_MARGIN;
        let top_inset = target.safeAreaInsets().top;
        let y = if target.isFlipped() {
            top_inset + margin
        } else {
            target.bounds().height() - top_inset - height - margin
        };
        rect(target.bounds().width() - width - margin, y, width, height)
    }

    fn screen_frame(frame: NSRect, view: &NSView, window: &NSWindow) -> NSRect {
        window.convertRectToScreen(view.convertRect_toView(frame, None))
    }

    pub(super) fn refit_floating_surface(&self, animated: bool) {
        let Some(surface) = self.floating_surface() else { return };
        if surface.is_dismissing() {
            return;
        }
        let Some(window) = self.window() else { return };
        let Some(target) = window.contentView() else { return };
        let width =
            surface.preferred_width().min((target.bounds().width() - 2.0 * PanelMetrics::FLOATING_MARGIN).max(200.0));
        let frame = self.floating_frame(&surface, &target, width, self.floating_height_cap(&target));
        let resting = Self::screen_frame(frame, &target, &window);
        let sliver = rect(
            resting.min_x(),
            resting.max_y() - Top::POUR_SLIVER_HEIGHT,
            resting.width(),
            Top::POUR_SLIVER_HEIGHT,
        );
        surface.configure_window_frames(resting, sliver, frame.height());
        surface.retarget_frame(resting, animated);
        self.set_floating_surface_frame(resting);
        surface.layoutSubtreeIfNeeded();
    }

    /// Task edits reshape the list's rows; a floating surface re-fits itself
    /// to the new content next turn, once the parse has repopulated the panel.
    pub fn refit_floating_surface_after_content_change(&self) {
        self.refit_floating_surface(true);
    }

    fn focus_floating_surface(&self, surface: &FloatingPanelSurface) {
        let Some(panel_window) = surface.window() else { return };
        panel_window.makeKeyWindow();
        let content = surface.content();
        if let Some(host) = downcast::<InspectorHostView>(&content) {
            host.focus_for_presentation();
        } else {
            panel_window.makeFirstResponder(Some(&content));
        }
    }

    pub(super) fn restore_floating_panel_window(&self) {
        let Some(surface) = self.floating_surface() else { return };
        let Some(panel_window) = self.floating_panel_window() else { return };
        let parent_matches = match (panel_window.parentWindow(), self.window()) {
            (Some(parent), Some(window)) => std::ptr::eq(&*parent, &*window),
            (None, None) => true,
            _ => false,
        };
        let surface_matches = surface.window().is_some_and(|window| std::ptr::eq(&*window, &**panel_window as &NSWindow));
        if !(parent_matches && surface_matches) {
            return;
        }
        panel_window.orderFrontRegardless();
        if !panel_window.isKeyWindow() {
            panel_window.makeKeyWindow();
        }
    }

    fn restore_floating_focus_and_close(&self) {
        if self.floating_surface().is_none() {
            return;
        }
        self.close_inspector(true);
    }

    /// `toggleTaskPanel()`.
    pub fn toggle_task_panel(&self) {
        // One floating inspector body owns Tasks, History, Document, and
        // Search. A second press closes only when Tasks is the active section.
        if self.floating_surface().is_some()
            && self.inspector_host().and_then(|host| host.selected_section()) == Some(InspectorSection::Tasks)
        {
            self.close_task_panel();
            return;
        }
        let panel = self.task_panel().unwrap_or_else(|| TaskPanelView::new_current(self.mtm()));
        if self.task_panel().is_none() {
            let delegate: Weak<dyn TaskPanelDelegate> = Rc::downgrade(&self.delegates()) as _;
            panel.set_delegate(Some(delegate));
            panel.set_style_sheet(self.active_style_sheet());
            let weak = ObjcWeak::new(self);
            panel.set_on_content_size_change(Some(Rc::new(move || {
                if let Some(this) = weak.load() {
                    this.refit_floating_surface(true);
                }
            })));
            let weak = ObjcWeak::new(self);
            panel.set_on_immediate_content_size_change(Some(Rc::new(move || {
                if let Some(this) = weak.load() {
                    this.refit_floating_surface(false);
                }
            })));
            self.set_task_panel(Some(panel.clone()));
        }
        let parsed = self.markdown_document().parsed();
        panel.set_tasks(parsed.tasks.clone());
        panel.set_headings(parsed.headings.clone());
        let weak = ObjcWeak::new(self);
        panel.set_on_close(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.close_task_panel();
            }
        })));
        // Build the final row model before the floating surface measures its
        // target. Measuring while the empty model is still installed reserves
        // the empty-state height and leaves dead space after the rows arrive.
        panel.reload();
        self.show_in_inspector(&panel, InspectorSection::Tasks);
        self.progress_ring().set_is_active(true);
    }

    /// `⌘N` normally belongs to the application New Document command. While
    /// Tasks is the active inspector, it is the panel's quick-add command.
    pub fn handle_new_document_command(&self) -> bool {
        if self.floating_surface().is_none()
            || self.inspector_host().and_then(|host| host.selected_section()) != Some(InspectorSection::Tasks)
        {
            return false;
        }
        let Some(task_panel) = self.task_panel() else { return false };
        task_panel.begin_new_task_for_command();
        true
    }

    /// Closes the panel from any of its doors — Esc, the header close button,
    /// a second ring press, or a click outside the glass.
    pub fn close_task_panel(&self) {
        if self.task_panel().is_none() {
            return;
        }
        // The ring settles first, so the glyph is already un-lit while the
        // glass pours back up toward the toolbar edge.
        self.progress_ring().set_is_active(false);
        self.close_inspector(true);
        self.refresh_toolbar_selection_state();
    }

    /// `title` names the surface after the command that opened it; without
    /// one the header keeps the section's generic name.
    pub fn install_trailing(&self, view: &NSView, title: Option<&str>) {
        self.show_in_inspector(view, InspectorSection::Context);
        if let Some(title) = title
            && let Some(host) = self.inspector_host()
        {
            host.set_title(title, InspectorSection::Context);
        }
    }

    /// `dismissTrailing(_:)`.
    pub fn dismiss_trailing(&self, view: &NSView) {
        if let Some(host) = self.inspector_host() {
            host.remove_content_view(view, InspectorSection::Context);
        }
        if self.inspector_host().map(|host| host.has_content()) != Some(true) {
            self.close_inspector(true);
        } else {
            self.refresh_toolbar_selection_state();
        }
    }

    /// `showInInspector(_:section:)`.
    pub fn show_in_inspector(&self, view: &NSView, section: InspectorSection) {
        // A close can still be springing toward the toolbar when the command
        // is invoked again. That surface will remove itself on arrival, so it
        // cannot safely host the reopened panel; retire it and create a fresh
        // surface whose lifecycle belongs to this presentation.
        if self.floating_surface().map(|surface| surface.is_dismissing()) == Some(true) {
            self.remove_floating_surface();
        }
        let host = match self.inspector_host() {
            Some(host) => host,
            None => {
                let created = InspectorHostView::new(RECT_ZERO, self.mtm());
                // The host is chrome like any panel and has to be born in the
                // document's theme.
                created.set_style_sheet(self.active_style_sheet());
                let weak = ObjcWeak::new(self);
                created.set_on_close(Some(Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.close_inspector(true);
                    }
                })));
                let weak = ObjcWeak::new(self);
                created.set_on_selection_change(Some(Rc::new(move |section| {
                    if let Some(this) = weak.load() {
                        this.inspector_selection_did_change(section);
                    }
                })));
                self.set_inspector_host(Some(created.clone()));
                created
            }
        };
        if self.floating_surface().is_none() {
            let first_responder = self.window().and_then(|window| window.firstResponder());
            let restore_view = first_responder.and_then(|responder| downcast::<NSView>(&responder));
            self.set_floating_focus_restore_view(restore_view.as_deref());
            host.set_content(view, section);
            let surface = FloatingPanelSurface::new(self.active_style_sheet(), &host, self.mtm());
            let weak = ObjcWeak::new(self);
            surface.set_on_close(Some(Rc::new(move || {
                if let Some(this) = weak.load() {
                    this.close_inspector(true);
                }
            })));
            self.present_floating_surface(&surface);
        } else {
            // Switching sections reuses the same glass body and header. The
            // document never enters a split-view resize path.
            host.set_content(view, section);
            if let Some(surface) = self.floating_surface() {
                surface.layoutSubtreeIfNeeded();
            }
            self.refit_floating_surface(true);
            let surface = self.floating_surface().expect("floatingSurface!");
            self.focus_floating_surface(&surface);
        }
        if section == InspectorSection::Tasks {
            self.progress_ring().set_is_active(true);
        }
        self.refresh_toolbar_selection_state();
    }

    fn inspector_selection_did_change(&self, section: Option<InspectorSection>) {
        self.cancel_sibling_search();
        if section != Some(InspectorSection::Search) || !self.sibling_search_active() {
            return;
        }
        let Some(query) = self.find_bar().map(|bar| bar.current_query()) else { return };
        self.run_find(query, false, true);
    }

    /// Closing is the arrival run backwards: the panel folds up toward the
    /// toolbar control that opened it, and only then does the pane give its
    /// width back to the document.
    pub fn close_inspector(&self, restoring_focus: bool) {
        self.cancel_sibling_search();
        if self.floating_surface().is_none() {
            self.refresh_toolbar_selection_state();
            return;
        }
        self.progress_ring().set_is_active(false);
        let repairs: Vec<Box<dyn Fn()>> = self
            .document_panes()
            .iter()
            .map(|pane| Box::new(pane.text_view().make_viewport_repair()) as Box<dyn Fn()>)
            .collect();
        *self.ivars().floating_dismiss_viewport_repairs.borrow_mut() = repairs;
        let restore = self.floating_focus_restore_view();
        self.dismiss_floating_surface();
        if restoring_focus {
            let restore_in_window = restore.filter(|view| match (view.window(), self.window()) {
                (Some(a), Some(b)) => std::ptr::eq(&*a, &*b),
                (None, None) => true,
                _ => false,
            });
            if let Some(restore) = restore_in_window {
                if let Some(window) = self.window() {
                    window.makeFirstResponder(Some(&restore));
                }
            } else if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self.primary_container().text_view()));
            }
        }
        self.set_floating_focus_restore_view(None);
        self.refresh_toolbar_selection_state();
    }

    /// Keeps auxiliary windows (timeline, compare, lightbox) alive for as
    /// long as this document window is.
    pub fn retain_timeline(&self, controller: &NSWindowController) {
        self.ivars().auxiliary_windows.borrow_mut().push(controller.retain());
        // Selector form, so the observation can unregister itself.
        // SAFETY: `auxiliaryWindowWillClose:` takes one `NSNotification`.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(auxiliaryWindowWillClose:),
                Some(NSWindowWillCloseNotification),
                controller.window().as_deref().map(|window| window as &objc2::runtime::AnyObject),
            );
        }
    }

    /// `@objc private func auxiliaryWindowWillClose(_:)`.
    pub(super) fn auxiliary_window_will_close(&self, notification: &NSNotification) {
        let Some(window) = notification.object().and_then(|object| downcast::<NSWindow>(&object)) else { return };
        // SAFETY: removes this controller's own registration.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver_name_object(
                self,
                Some(NSWindowWillCloseNotification),
                Some(&window),
            );
        }
        self.ivars().auxiliary_windows.borrow_mut().retain(|controller| {
            !controller.window().is_some_and(|candidate| std::ptr::eq(&*candidate, &*window))
        });
    }

    /// The host fills the pane, top to bottom. Docked columns only — a panel
    /// never sizes the window.
    #[allow(dead_code)]
    fn install(&self, view: &NSView, pane: &NSView) {
        for existing in pane.subviews() {
            existing.removeFromSuperview();
        }
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        pane.addSubview(view);
        activate(&[
            view.leadingAnchor().constraintEqualToAnchor(&pane.leadingAnchor()),
            view.trailingAnchor().constraintEqualToAnchor(&pane.trailingAnchor()),
            view.topAnchor().constraintEqualToAnchor(&pane.topAnchor()),
            view.bottomAnchor().constraintEqualToAnchor(&pane.bottomAnchor()),
        ]);
    }
}

use objc2::Message;
