//! `// MARK: - Find (§9.4)` of `DocumentWindowController.swift`: the find
//! bar's liquid entrance and exit, the search inspector, and running find.

use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2_app_kit::{NSLayoutConstraint, NSView, NSWindowOrderingMode};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSString};
use objc2_quartz_core::{
    CAAnimationGroup, CAKeyframeAnimation, CAMediaTiming, CATransaction, CATransform3D, CATransform3DIdentity,
};
use upleft_render::appkit_compat::{RectExt, main_async};
use upleft_render::motion::{self, Curve};
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use objc2::DefinedClass as _;
use crate::panels::appkit_support::Presentation as _;
use super::DocumentWindowController;
use super::derived_ui::{numbers, set_keyframe_values, transform_value};
use crate::panels::appkit_support::{IDENTITY, activate};
use crate::panels::find_bar_view::{FindBarDelegate, FindBarDensity, FindBarView, Presentation};
use crate::panels::inspector_host_view::InspectorSection;
use crate::panels::panel_chrome::PanelMetrics;
use crate::panels::search_inspector_view::SearchInspectorView;
use crate::support::find_engine::{FindEngine, FindQuery};

impl DocumentWindowController {
    // MARK: - Find (§9.4)

    /// `showFindBar(replace:queryAfterFocus:)`.
    pub fn show_find_bar(&self, replace: bool, query_after_focus: Option<FindQuery>) {
        let mtm = self.mtm();
        // Reopening while the previous pill is travelling owns the same
        // visual lane. Cancel the stale exit before installing the new bar;
        // otherwise its completion could retire the freshly reopened view.
        // (The cell is read into a local: an `if let` scrutinee's borrow
        // would live through the block, which writes the same cell.)
        let exiting_find_bar = self.ivars().exiting_find_bar.borrow().clone();
        if let Some(exiting) = exiting_find_bar {
            let generation = self.ivars().find_bar_exit_generation.get().wrapping_add(1);
            self.ivars().find_bar_exit_generation.set(generation);
            Self::retire(&exiting);
            let exiting_inspector = self.ivars().exiting_search_inspector.borrow().clone();
            if let Some(inspector) = exiting_inspector
                && self
                    .inspector_host()
                    .and_then(|host| host.content_for(InspectorSection::Search))
                    .is_some_and(|content| std::ptr::eq(&*content, &**inspector as &NSView))
            {
                if let Some(host) = self.inspector_host() {
                    host.remove_content(InspectorSection::Search);
                }
                if self.inspector_host().map(|host| host.has_content()) != Some(true) {
                    self.close_inspector(false);
                }
            }
            *self.ivars().exiting_find_bar.borrow_mut() = None;
            *self.ivars().exiting_search_inspector.borrow_mut() = None;
        }
        let viewport_repairs: Vec<Box<dyn Fn()>> = if query_after_focus.as_ref().map(|query| query.is_empty()) != Some(false) {
            self.document_panes()
                .iter()
                .map(|pane| Box::new(pane.text_view().make_viewport_repair()) as Box<dyn Fn()>)
                .collect()
        } else {
            Vec::new()
        };
        let retained_sibling_results = self.search_results();
        if self.search_inspector().is_some() {
            self.dismiss_find_bar();
            // Reduce Motion finishes dismissal synchronously, before the
            // ordinary Find bar below exists. Preserve the sibling session
            // across that handoff just as the animated path does naturally.
            if self.search_results().is_none() {
                self.set_search_results(retained_sibling_results);
            }
        }

        let existing_query = self.find_bar().map(|bar| bar.current_query());
        let bar = match self.find_bar() {
            Some(bar) => bar,
            None => {
                let root = self.root_view();
                let created = FindBarView::new(self.active_style_sheet(), Presentation::Bar, mtm);
                let delegate: Weak<dyn FindBarDelegate> = Rc::downgrade(&self.delegates()) as _;
                created.set_delegate(Some(delegate));
                self.set_find_bar(Some(created.clone()));
                created.prepare_for_liquid_entrance();
                created.setTranslatesAutoresizingMaskIntoConstraints(false);
                root.addSubview_positioned_relativeTo(&created, NSWindowOrderingMode::Above, None);
                // Search floats above a stable document. Opening transient
                // chrome must not move the page under the reader.
                let width = created.widthAnchor().constraintEqualToConstant(FindBarDensity::BAR_WIDTH);
                width.setPriority(objc2_app_kit::NSLayoutPriorityDefaultHigh);
                activate(&[
                    width,
                    created
                        .widthAnchor()
                        .constraintLessThanOrEqualToAnchor_constant(&root.widthAnchor(), -2.0 * PanelMetrics::INSET),
                    created.centerXAnchor().constraintEqualToAnchor(&root.centerXAnchor()),
                    created
                        .topAnchor()
                        .constraintEqualToAnchor_constant(&root.safeAreaLayoutGuide().topAnchor(), 14.0),
                ]);
                created.setWantsLayer(true);
                if let Some(content) = self.window().and_then(|window| window.contentView()) {
                    content.layoutSubtreeIfNeeded();
                }
                let source = self.toolbar_find_button().map(|button| {
                    let bounds = button.bounds();
                    button.convertPoint_toView(NSPoint::new(bounds.mid_x(), bounds.mid_y()), None)
                });
                created.play_liquid_entrance(source);
                created
            }
        };

        bar.set_shows_replace(replace);
        // Keep ⌘F and ⌘E distinct: Find opens a clean search field, while Use
        // Selection for Find explicitly supplies the selected query.
        let requested_query = query_after_focus.or(existing_query).unwrap_or_default();
        bar.set_query_text(&requested_query.text, false);
        if !requested_query.is_empty() {
            self.run_find(requested_query.clone(), true, true);
        }
        bar.focus_search_field(false);
        // AppKit may seed the field once more as first responder activation
        // settles. Reassert the command's query on the next settled layout
        // turn; otherwise ⌘F becomes "find the selection" again.
        let weak = ObjcWeak::new(self);
        let weak_bar = ObjcWeak::new(&*bar);
        main_async(move || {
            let Some(bar) = weak_bar.load() else { return };
            if bar.window().is_none() {
                return;
            }
            if let Some(window) = bar.window() {
                window.layoutIfNeeded();
            }
            bar.set_query_text(&requested_query.text, false);
            if let Some(this) = weak.load()
                && !requested_query.is_empty()
                && this.find_bar().is_some_and(|current| std::ptr::eq(&*current, &*bar))
            {
                this.run_find(requested_query, true, true);
            }
        });
        self.refresh_toolbar_selection_state();
        // `defer { viewportRepairs.forEach { $0() } }`
        for repair in &viewport_repairs {
            repair();
        }
    }

    /// `showFindInspector(replace:)`.
    pub fn show_find_inspector(&self, replace: bool) {
        let retained_sibling_results = self.search_results();
        if self.search_inspector().is_none() && self.find_bar().is_some() {
            self.dismiss_find_bar();
            // The Reduce Motion path completes before the sibling inspector
            // below is installed. Keep the retained panel alive so reopening
            // can reattach it instead of silently starting a new session.
            if self.search_results().is_none() {
                self.set_search_results(retained_sibling_results);
            }
        }

        let inspector = match self.search_inspector() {
            Some(inspector) => inspector,
            None => {
                let created = SearchInspectorView::new(self.active_style_sheet(), self.mtm());
                let delegate: Weak<dyn FindBarDelegate> = Rc::downgrade(&self.delegates()) as _;
                created.find_bar().set_delegate(Some(delegate));
                self.set_search_inspector(Some(created.clone()));
                self.set_find_bar(Some(created.find_bar()));
                created
            }
        };
        inspector.set_shows_replace(replace);
        self.show_in_inspector(&inspector, InspectorSection::Search);
        inspector.find_bar().focus_search_field(true);
    }

    /// `dismissFindBar()`.
    pub fn dismiss_find_bar(&self) {
        self.cancel_sibling_search();
        let Some(leaving) = self.find_bar() else { return };

        let generation = self.ivars().find_bar_exit_generation.get().wrapping_add(1);
        self.ivars().find_bar_exit_generation.set(generation);
        let leaving_inspector = self.search_inspector();
        *self.ivars().exiting_find_bar.borrow_mut() = Some(leaving.clone());
        *self.ivars().exiting_search_inspector.borrow_mut() = leaving_inspector.clone();

        // Clear the command-facing references now, but keep the actual view
        // hierarchy, inspector shell, and match highlights alive until the
        // last exit frame.
        self.set_find_bar(None);
        self.set_search_inspector(None);
        leaving.prepare_for_liquid_exit();

        let weak = ObjcWeak::new(self);
        let weak_leaving = ObjcWeak::new(&*leaving);
        let weak_inspector = leaving_inspector.as_deref().map(ObjcWeak::new);
        let finish = move || {
            let (Some(this), Some(leaving)) = (weak.load(), weak_leaving.load()) else { return };
            let inspector = weak_inspector.as_ref().and_then(ObjcWeak::load);
            this.finish_find_bar_dismissal(&leaving, inspector, generation);
        };
        if self.active_style_sheet().reduce_motion || unsafe { leaving.superview() }.is_none() {
            finish();
        } else {
            self.animate_find_bar_exit(&leaving, Box::new(finish));
        }
    }

    fn animate_find_bar_exit(&self, bar: &FindBarView, completion: Box<dyn Fn()>) {
        let Some(layer) = bar.layer() else {
            completion();
            return;
        };
        let transform = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        let destination = self.find_bar_source_transform(bar);
        let presentation = layer.__presentation();
        set_keyframe_values(
            &transform,
            &[
                transform_value(
                    presentation
                        .as_ref()
                        .map(|presentation| presentation.transform())
                        .unwrap_or(unsafe { CATransform3DIdentity }),
                ),
                transform_value(CATransform3D::new_translation(0.0, 2.0, 0.0).concat(CATransform3D::new_scale(0.985, 0.98, 1.0))),
                transform_value(destination),
            ],
        );
        transform.setKeyTimes(Some(&numbers(&[NSNumber::new_isize(0), NSNumber::new_f64(0.24), NSNumber::new_isize(1)])));
        transform.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
            motion::timing(Curve::EaseOut),
            motion::timing(Curve::Structural),
        ])));
        let opacity = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let from_opacity = presentation.as_ref().map(|presentation| presentation.opacity()).unwrap_or(1.0);
        let opacity_values: [Retained<NSNumber>; 3] =
            [NSNumber::new_f32(from_opacity), NSNumber::new_f32(0.8), NSNumber::new_f32(0.0)];
        set_keyframe_values(
            &opacity,
            &opacity_values.map(|value| Retained::into_super(Retained::into_super(Retained::into_super(value)))),
        );
        opacity.setKeyTimes(Some(&numbers(&[NSNumber::new_isize(0), NSNumber::new_f64(0.28), NSNumber::new_isize(1)])));
        opacity.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
            motion::timing(Curve::EaseOut),
            motion::timing(Curve::Structural),
        ])));
        let group = CAAnimationGroup::new();
        group.setAnimations(Some(&NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(transform)),
            Retained::into_super(Retained::into_super(opacity)),
        ])));
        group.setDuration(motion::DELIBERATE);
        CATransaction::begin();
        let completion = RcBlock::new(move || completion());
        unsafe { CATransaction::setCompletionBlock(Some(&completion)) };
        layer.setOpacity(0.0);
        layer.addAnimation_forKey(&group, Some(&NSString::from_str("find-bar-exit")));
        CATransaction::commit();
    }

    fn finish_find_bar_dismissal(&self, bar: &FindBarView, inspector: Option<Retained<SearchInspectorView>>, generation: isize) {
        let exiting_is_bar = self
            .ivars()
            .exiting_find_bar
            .borrow()
            .as_ref()
            .is_some_and(|exiting| std::ptr::eq(&**exiting, bar));
        if !(generation == self.ivars().find_bar_exit_generation.get() && exiting_is_bar) {
            return;
        }
        // Removing the focused search field can make AppKit reveal the live
        // editor selection. Capture at the last exit frame, not when dismissal
        // starts, so any scrolling during the animation remains intentional.
        let panes = self.document_panes();
        let viewport_repairs: Vec<Box<dyn Fn()>> =
            panes.iter().map(|pane| Box::new(pane.text_view().make_viewport_repair()) as Box<dyn Fn()>).collect();
        let viewport_xs: Vec<f64> = panes.iter().map(|pane| pane.scroll_view().contentView().bounds().origin.x).collect();

        Self::retire(bar);
        let exiting_inspector = self.ivars().exiting_search_inspector.borrow().clone();
        if let Some(inspector) = &inspector
            && exiting_inspector.as_ref().is_some_and(|exiting| std::ptr::eq(&**exiting, &**inspector))
            && self
                .inspector_host()
                .and_then(|host| host.content_for(InspectorSection::Search))
                .is_some_and(|content| std::ptr::eq(&*content, &***inspector as &NSView))
        {
            if let Some(host) = self.inspector_host() {
                host.remove_content(InspectorSection::Search);
            }
            if self.inspector_host().map(|host| host.has_content()) != Some(true) {
                self.close_inspector(false);
            }
        }
        let same_inspector = match (self.ivars().exiting_search_inspector.borrow().as_ref(), inspector.as_ref()) {
            (Some(a), Some(b)) => std::ptr::eq(&**a, &**b),
            (None, None) => true,
            _ => false,
        };
        if same_inspector {
            *self.ivars().exiting_search_inspector.borrow_mut() = None;
        }
        // Keep ⌘G/Find Next's query in the session, but remove visible marks
        // only after the pill and its inspector shell have fully left.
        if self.find_bar().is_none() && self.search_inspector().is_none() {
            self.set_search_results(None);
            for pane in self.document_panes() {
                pane.text_view().set_search_hits(Vec::new());
                pane.text_view().set_current_search_hit(None);
            }
            self.refresh_density_bands(None);
        }
        *self.ivars().exiting_find_bar.borrow_mut() = None;
        self.refresh_toolbar_selection_state();
        // `defer { viewportRepairs.forEach { $0() }; restoreHorizontalPosition() }`
        for repair in &viewport_repairs {
            repair();
        }
        for (pane, x) in panes.iter().zip(viewport_xs) {
            let clip = pane.scroll_view().contentView();
            clip.scrollToPoint(NSPoint::new(x, clip.bounds().origin.y));
            pane.scroll_view().reflectScrolledClipView(&clip);
        }
    }

    fn find_bar_source_transform(&self, bar: &FindBarView) -> CATransform3D {
        let button = self.toolbar_find_button().filter(|button| match (button.window(), bar.window()) {
            (Some(a), Some(b)) => std::ptr::eq(&*a, &*b),
            (None, None) => true,
            _ => false,
        });
        let Some(button) = button.filter(|_| bar.bounds().width() > 1.0 && bar.bounds().height() > 1.0) else {
            return CATransform3D::new_translation(0.0, 9.0, 0.0).concat(CATransform3D::new_scale(0.93, 0.84, 1.0));
        };
        let button_bounds = button.bounds();
        let source = button.convertPoint_toView(NSPoint::new(button_bounds.mid_x(), button_bounds.mid_y()), None);
        let bar_bounds = bar.bounds();
        let destination = bar.convertPoint_toView(NSPoint::new(bar_bounds.mid_x(), bar_bounds.mid_y()), None);
        // Avoid a 10-12x backdrop stretch. Begin as a compact lens, still
        // centred on the invoking button, then let the material travel and
        // widen together.
        let scale_x = 0.28f64.max(0.34f64.min(button_bounds.width() / bar_bounds.width()));
        let scale_y = 0.78f64.max(0.94f64.min(button_bounds.height() / bar_bounds.height()));
        CATransform3D::new_translation(source.x - destination.x, source.y - destination.y, 0.0)
            .concat(CATransform3D::new_scale(scale_x, scale_y, 1.0))
    }

    /// Removes the transient overlay. Repeated close commands are harmless
    /// because the state is cleared before the exit animation starts.
    fn retire(bar: &FindBarView) {
        bar.removeFromSuperview();
        bar.setAlphaValue(1.0);
        if let Some(layer) = bar.layer() {
            layer.removeAllAnimations();
        }
        if let Some(layer) = bar.layer() {
            layer.setTransform(unsafe { CATransform3DIdentity });
        }
        if let Some(layer) = bar.layer() {
            layer.setAffineTransform(IDENTITY);
        }
    }

    /// `applyFindQuery(_:)`.
    pub fn apply_find_query(&self, query: FindQuery) {
        self.run_find(query, true, true);
    }

    /// `runFind(_:scrollToMatch:highlightAll:)`.
    pub fn run_find(&self, query: FindQuery, scroll_to_match: bool, highlight_all: bool) {
        if let Some(item) = self.ivars().find_refresh_work_item.borrow().as_ref() {
            item.cancel();
        }
        let source = self.container_text_view();
        let text = self.markdown_document().text();
        self.find_session().borrow_mut().update(query.clone(), &text, source.top_visible_offset());
        // A hit inside a folded or elided range forces that range visible; the
        // text view owns that rule (§14's four-way interaction).
        let (matches, current, status) = {
            let session = self.find_session().borrow();
            (session.matches().to_vec(), session.current_match(), session.status_text())
        };
        for pane in self.document_panes() {
            if highlight_all {
                pane.text_view().set_search_hits(matches.clone());
            }
            pane.text_view().set_current_search_hit(current);
        }
        if let Some(bar) = self.find_bar() {
            bar.set_status_text(&status);
        }
        if let Some(bar) = self.find_bar() {
            bar.set_is_query_valid(FindEngine::is_valid(&query));
        }
        self.refresh_sibling_search(&query);
        self.refresh_density_bands(None);
        let current = self.find_session().borrow().current_match();
        if scroll_to_match && let Some(found) = current {
            source.scroll_to_offset(found.location, ScrollPosition::Center, false);
            self.synchronize_panes(&source, false, false);
        }
    }

    /// `advanceFind(forward:)`.
    pub fn advance_find(&self, forward: bool) {
        // With the bar closed, ⌘G still advances but first re-checks the query
        // against the buffer so a stale match set cannot point at moved text.
        if self.find_bar().is_none() && !self.find_session().borrow().query().is_empty() {
            let query = self.find_session().borrow().query().clone();
            self.run_find(query, false, false);
        }
        let Some(found) = self.find_session().borrow_mut().advance(forward) else { return };
        let source = self.container_text_view();
        for pane in self.document_panes() {
            pane.text_view().set_current_search_hit(Some(found));
        }
        if let Some(bar) = self.find_bar() {
            bar.set_status_text(&self.find_session().borrow().status_text());
        }
        self.record_jump(found.location, "Search hit");
        source.scroll_to_offset(found.location, ScrollPosition::Center, true);
        self.synchronize_panes(&source, false, false);
    }
}

#[allow(unused_imports)]
use NSLayoutConstraint as _;
