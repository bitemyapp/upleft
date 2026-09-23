//! `// MARK: - Derived UI`, `// MARK: - Autosave` and `// MARK: - External
//! changes (§8.1)` of `DocumentWindowController.swift`: the coalesced
//! panel/metrics refresh, density bands and breadcrumb, change marks, and the
//! change-summary and conflict bars with their Core Animation transitions.

use std::rc::Rc;
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSAnimationContext, NSWindowOrderingMode};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSString, NSValue};
use objc2_quartz_core::{
    CAAnimation, CAAnimationGroup, CABasicAnimation, CAKeyframeAnimation, CAMediaTiming, CAMediaTimingFunction,
    CATransaction, CATransform3D, CATransform3DIdentity, kCAFillModeForwards, kCAMediaTimingFunctionEaseIn,
    kCAMediaTimingFunctionEaseInEaseOut, kCAMediaTimingFunctionEaseOut,
};
use upleft_core::{NSRange, ParsedDocument, ReadingMetrics};
use upleft_core::metrics::Metrics;
use upleft_foundation::date::Date;
use upleft_render::appkit_compat::{RectExt, WorkItem, main_async};
use upleft_render::view::density_gutter_view::DensityGutterView;
use upleft_render::view::density_outline_window::DensityOutlineEntry;

use objc2::DefinedClass as _;
use objc2_app_kit::NSAnimatablePropertyContainer as _;
use objc2_quartz_core::NSValueCATransform3DAdditions as _;
use super::DocumentWindowController;
use crate::ai::markdown_document::{ExternalEvent, Phase, PresentationState};
use crate::panels::appkit_support::activate;
use crate::panels::breadcrumb_view::Crumb;
use crate::panels::change_summary_bar_view::{ChangeSummaryBarDelegate, ChangeSummaryBarView, Summary};
use crate::panels::conflict_bar_view::{ConflictBarDelegate, ConflictBarView};
use crate::support::find_engine::FindQuery;
use crate::support::preferences::Preferences;

impl DocumentWindowController {
    // MARK: - Derived UI

    /// `scheduleDerivedUIRefresh(immediate:)`.
    pub fn schedule_derived_ui_refresh(&self, immediate: bool) {
        if let Some(item) = self.ivars().derived_ui_refresh_work_item.borrow().as_ref() {
            item.cancel();
        }
        if immediate {
            self.refresh_derived_ui();
            return;
        }
        let weak = ObjcWeak::new(self);
        let work = WorkItem::new(move || {
            if let Some(this) = weak.load() {
                this.refresh_derived_ui();
            }
        });
        *self.ivars().derived_ui_refresh_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(0.12);
    }

    /// `scheduleFindRefresh()`.
    pub fn schedule_find_refresh(&self) {
        if self.current_find_query().is_empty() {
            return;
        }
        if let Some(item) = self.ivars().find_refresh_work_item.borrow().as_ref() {
            item.cancel();
        }
        let weak = ObjcWeak::new(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            if this.current_find_query().is_empty() {
                return;
            }
            this.run_find(this.current_find_query(), true, true);
        });
        *self.ivars().find_refresh_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(0.04);
    }

    /// Find-as-you-type path. Each keystroke re-runs a fresh match over the
    /// whole document on the main thread; coalescing them into one run per
    /// idle tick makes typing in the search field cheap.
    pub fn schedule_find_query(&self, query: FindQuery) {
        self.stage_sibling_search(&query);
        if query.is_empty() {
            self.run_find(query, true, true); // clearing the field must take effect immediately
            return;
        }
        if let Some(item) = self.ivars().find_refresh_work_item.borrow().as_ref() {
            item.cancel();
        }
        let weak = ObjcWeak::new(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            this.run_find(query, true, true);
        });
        *self.ivars().find_refresh_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(0.06);
    }

    // MARK: - Autosave

    /// Schedules a save after a brief idle period when the document is dirty
    /// and autosave is enabled. Repeated edits reset the timer, so a rapid
    /// typing burst produces one save, not one per keystroke.
    pub(super) fn schedule_autosave(&self) {
        // scheduleAutosave runs exactly when the buffer turns dirty, i.e. on
        // fresh work. A previous Discard decision covered the buffer as it
        // existed then; new edits are new work and re-arm implicit saves.
        self.ivars().implicit_save_suppressed.set(false);
        if !(Preferences::shared().values().autosave_enabled && self.markdown_document().url().is_some()) {
            return;
        }
        if let Some(item) = self.ivars().autosave_work_item.borrow().as_ref() {
            item.cancel();
        }
        let weak = ObjcWeak::new(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            if !(this.markdown_document().is_dirty() && !this.ivars().implicit_save_suppressed.get()) {
                return;
            }
            let _ = this.save_document();
        });
        *self.ivars().autosave_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(2.0);
    }

    /// `refreshDerivedUI()`.
    pub fn refresh_derived_ui(&self) {
        let parsed = self.markdown_document().parsed();
        let source = self.container_text_view();
        let metrics = self.section_metrics(&parsed);
        if let Some(task_panel) = self.task_panel() {
            task_panel.set_tasks(parsed.tasks.clone());
            task_panel.set_headings(parsed.headings.clone());
            task_panel.reload();
            self.refit_floating_surface(true);
        }
        if let Some(editor) = self.front_matter_editor() {
            editor.set_document(parsed.clone());
        }
        if let Some(panel) = self.asset_doctor_panel() {
            self.configure_asset_doctor(&panel);
        }
        self.refresh_document_lens_if_visible();
        self.refresh_diagnostics_panels();
        self.refresh_visual_debugger_if_visible();
        self.refresh_review_panel_if_visible();
        let completed_tasks = parsed.tasks.iter().fold(0isize, |count, task| if task.is_checked { count + 1 } else { count });
        self.progress_ring().set_progress(completed_tasks, parsed.tasks.len() as isize);
        self.refresh_toolbar_selection_state();

        // Path existence is stable across local edits; wipe only on external
        // writes and document hops (see handleExternalEvent / open).
        self.refresh_density_bands(Some(&metrics));
        self.ivars().last_breadcrumb_heading_index.set(isize::MIN);
        self.refresh_breadcrumb();
        let mut state = self.markdown_document().state();
        state.zoom_level = source.zoom_level();
        state.folded_headings = source.folded_heading_slugs().into_iter().collect();
        self.markdown_document().set_state(state);
        self.update_focus_dimming_views();
    }

    fn section_metrics(&self, parsed: &ParsedDocument) -> Vec<ReadingMetrics> {
        let id = Arc::as_ptr(&parsed.root) as usize;
        if self.ivars().cached_metrics_document_id.get() == Some(id) {
            return self.ivars().cached_section_metrics.borrow().clone();
        }
        let metrics = Metrics::section_metrics(parsed);
        self.ivars().cached_metrics_document_id.set(Some(id));
        *self.ivars().cached_section_metrics.borrow_mut() = metrics.clone();
        self.ivars().cached_word_count.set(metrics.iter().fold(0, |sum, metric| sum + metric.words));
        if self.ivars().cached_word_count.get() == 0 && parsed.length > 0 {
            self.ivars().cached_word_count.set(whitespace_separated_count(&self.markdown_document().text()));
        }
        metrics
    }

    /// Change marks go visited on departure or after a dwell, never on
    /// arrival — see `jumpChange`. Driven from the scroll path so "left the
    /// viewport" is something the reader actually did.
    pub fn note_visible_change_marks(&self) {
        let view = self.container_text_view();
        let visible = match view.enclosingScrollView() {
            Some(scroll_view) => scroll_view.documentVisibleRect(),
            None => view.visibleRect(),
        };
        let origin = view.textContainerOrigin();
        let top = view.top_visible_offset();
        let bottom =
            view.source_offset_at(NSPoint::new(origin.x + 1.0, (visible.max_y() - 1.0).max(visible.min_y())));
        self.markdown_document()
            .changes()
            .note_visible_range(NSRange { location: top.min(bottom), length: (bottom - top).abs() }, Date::now());
        if let Some(identity) = self.toolbar_document_identity_view() {
            identity.set_has_external_changes(self.markdown_document().changes().unread_count() > 0);
        }
    }

    /// `refreshDensityBands(metrics:)`.
    pub fn refresh_density_bands(&self, metrics: Option<&[ReadingMetrics]>) {
        self.primary_container().refresh_margin_notes();
        if let Some(split) = self.split_container() {
            split.refresh_margin_notes();
        }
        let parsed = self.markdown_document().parsed();
        if metrics.is_none() {
            let _ = self.section_metrics(&parsed);
        }
        let word_count = self.ivars().cached_word_count.get();
        let read_minutes = 1.max((word_count + 199) / 200);
        // The gutter's hover summary is the *only* place these two live: the
        // status bar used to repeat them permanently, which is what a calm
        // document surface is meant not to do.
        let gutter = self.density_gutter_view();
        gutter.set_metrics_summary(format!("{word_count} words · {read_minutes} min read"));
        self.status_bar_view().set_has_file_url(self.markdown_document().url().is_some());
        let search_hits: Vec<NSRange> = self.find_session().borrow().matches().to_vec();
        gutter.set_bands(DensityGutterView::bands_for(&parsed, &[], &search_hits));
        let length = 1.max(parsed.length) as CGFloat;
        let source = self.container_text_view();
        let current = self.visible_heading_index(source.top_visible_offset());
        let entries = parsed
            .headings
            .iter()
            .enumerate()
            .map(|(index, heading)| {
                DensityOutlineEntry::new(
                    heading.title.clone(),
                    heading.level,
                    heading.range.location as CGFloat / length,
                    Some(index) == current,
                )
            })
            .collect();
        gutter.set_outline_entries(entries);
        gutter.setNeedsDisplay(true);
    }

    /// `refreshBreadcrumb()`.
    pub fn refresh_breadcrumb(&self) {
        let source = self.container_text_view();
        self.breadcrumb_view().set_zoom_level(source.zoom_level());
        let offset = source.top_visible_offset();
        let parsed = self.markdown_document().parsed();
        let headings = &parsed.headings;
        let resolved = self.visible_heading_index(offset);
        let cache_key = resolved.map_or(-1, |index| index as isize);
        if cache_key == self.ivars().last_breadcrumb_heading_index.get() {
            return;
        }
        self.ivars().last_breadcrumb_heading_index.set(cache_key);
        let Some(mut index) = resolved.or(if headings.is_empty() { None } else { Some(0) }) else {
            self.breadcrumb_view().set_trail(Vec::new());
            return;
        };
        let mut trail: Vec<Crumb> = Vec::new();
        loop {
            let heading = &headings[index];
            trail.insert(0, Crumb::new(index as isize, &heading.title, heading.level));
            let Some(parent) = heading.parent_index else { break };
            index = parent as usize;
        }
        self.breadcrumb_view().set_trail(trail);
    }

    /// Last heading beginning at or before `offset`, resolved in logarithmic
    /// time. Scroll callbacks use this once and share the answer across
    /// chrome.
    pub fn visible_heading_index(&self, offset: isize) -> Option<usize> {
        let parsed = self.markdown_document().parsed();
        let headings = &parsed.headings;
        let mut low = 0usize;
        let mut high = headings.len();
        while low < high {
            let middle = (low + high) / 2;
            if headings[middle].range.location <= offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low > 0 { Some(low - 1) } else { None }
    }

    /// `refreshChangeDecorations()`.
    pub fn refresh_change_decorations(&self) {
        // Change tracking still powers summaries, navigation, persistence, and
        // accessibility. The coloured document marks and density-rail dots
        // are intentionally absent from the calm reading surface.
        self.primary_container().text_view().set_change_marks(Vec::new());
        if let Some(split) = self.split_container() {
            split.text_view().set_change_marks(Vec::new());
        }
        // Change tracking can be published from the external-write commit.
        // Rebuilding every density/outline band in that same main-actor turn
        // turns a tiny source insertion into a visible scroll hitch; the normal
        // derived-UI debounce coalesces it with the incoming parse commit.
        self.schedule_derived_ui_refresh(false);
    }

    /// "I have read all of these" — the review queue's one explicit exit.
    pub fn mark_changes_reviewed(&self) {
        let changes = self.markdown_document().changes();
        if changes.is_empty() {
            return;
        }
        let count = changes.count();
        changes.clear();
        self.dismiss_change_summary();
        if let Some(identity) = self.toolbar_document_identity_view() {
            identity.set_has_external_changes(false);
        }
        self.announce_transient_status(&format!(
            "Marked {count} change{} as reviewed",
            if count == 1 { "" } else { "s" }
        ));
    }

    // MARK: - External changes (§8.1)

    pub(super) fn handle_external_event(&self, event: &ExternalEvent) {
        match event {
            ExternalEvent::Applied { hunks } => {
                self.refresh_change_decorations();
                // The external parse commits asynchronously. Invalidate now;
                // the matching onReparse callback warms the new revision's
                // tokens.
                if let Some(resolver) = self.path_resolver() {
                    resolver.invalidate();
                }
                self.schedule_find_refresh();
                if hunks.is_empty() {
                    return;
                }
                if let Some(identity) = self.toolbar_document_identity_view() {
                    identity.set_has_external_changes(true);
                }
                self.show_change_summary(None);
            }
            ExternalEvent::Conflict(conflict) => {
                if let Some(identity) = self.toolbar_document_identity_view() {
                    identity.set_has_external_changes(true);
                }
                self.set_pending_conflict(Some(conflict.clone()));
                self.show_conflict_bar(&format!(
                    "Changed on disk — {} block{}",
                    conflict.changed_block_count,
                    if conflict.changed_block_count == 1 { "" } else { "s" }
                ));
            }
            ExternalEvent::FileRemoved => {
                if let Some(identity) = self.toolbar_document_identity_view() {
                    identity.set_document_state(PresentationState::new(
                        Phase::ChangedOnDisk,
                        None,
                        Some("File missing".to_owned()),
                    ));
                }
                self.show_conflict_bar("File was moved or deleted");
            }
            ExternalEvent::FileRestored => {
                self.dismiss_conflict_bar();
            }
        }
    }

    pub(super) fn present_unread_changes(&self) {
        if !(self.markdown_document().changes().unread_count() > 0) {
            return;
        }
        self.show_change_summary(None);
        self.refresh_change_decorations();
    }

    /// Where the floating change bar sits, measured from the top of the
    /// document container: clear of the breadcrumb's reserved lane.
    fn change_summary_top_inset(&self) -> CGFloat {
        14.0
    }

    /// The lane changes height when the breadcrumb is hidden — Focus mode —
    /// so the bar's offset is a constant that gets refreshed, not one set
    /// once.
    pub fn refresh_change_summary_top_inset(&self) {
        if let Some(constraint) = self.change_summary_top_constraint() {
            constraint.setConstant(self.change_summary_top_inset());
        }
    }

    /// Shows the change summary. With no message the bar describes the write
    /// itself from the tracker's marks; a message is for the events the marks
    /// cannot name, such as a rename.
    pub fn show_change_summary(&self, message: Option<&str>) {
        if let Some(item) = self.change_summary_dismiss_work_item() {
            item.cancel();
        }
        let root = self.root_view();
        if self.change_summary_bar().is_none() {
            let bar = ChangeSummaryBarView::new_current(self.mtm());
            let delegate: std::rc::Weak<dyn ChangeSummaryBarDelegate> = Rc::downgrade(&self.delegates()) as _;
            bar.set_delegate(Some(delegate));
            bar.set_style_sheet(self.active_style_sheet());
            self.set_change_summary_bar(Some(bar.clone()));
            bar.setTranslatesAutoresizingMaskIntoConstraints(false);
            root.addSubview_positioned_relativeTo(&bar, NSWindowOrderingMode::Above, None);
            let top = bar.topAnchor().constraintEqualToAnchor_constant(&root.topAnchor(), self.change_summary_top_inset());
            self.set_change_summary_top_constraint(Some(top.clone()));
            activate(&[top, bar.trailingAnchor().constraintEqualToAnchor_constant(&root.trailingAnchor(), -16.0)]);
        }
        self.refresh_change_summary_top_inset();
        if let Some(bar) = self.change_summary_bar() {
            let changes = self.markdown_document().changes();
            bar.configure(
                message,
                Summary::new(&changes.unread_marks(), self.markdown_document().storage().length() as isize),
            );
        }
        root.setNeedsLayout(true);
        root.layoutSubtreeIfNeeded();
        self.animate_change_summary_in_if_needed();
        self.schedule_change_summary_dismissal();
    }

    /// `showConflictBar(_:)`.
    pub fn show_conflict_bar(&self, message: &str) {
        if self.conflict_bar().is_none() {
            let bar = ConflictBarView::new_current(self.mtm());
            let delegate: std::rc::Weak<dyn ConflictBarDelegate> = Rc::downgrade(&self.delegates()) as _;
            bar.set_delegate(Some(delegate));
            bar.set_style_sheet(self.active_style_sheet());
            self.set_conflict_bar(Some(bar.clone()));
            let bar_stack = self.bar_stack();
            bar_stack.addArrangedSubview(&bar);
            bar.widthAnchor().constraintEqualToAnchor(&bar_stack.widthAnchor()).setActive(true);
        }
        if let Some(bar) = self.conflict_bar() {
            bar.set_message(message);
        }
        self.root_view().setNeedsLayout(true);
    }

    /// `dismissConflictBar()`.
    pub fn dismiss_conflict_bar(&self) {
        if let Some(bar) = self.conflict_bar() {
            self.bar_stack().removeArrangedSubview(&bar);
            bar.removeFromSuperview();
        }
        self.set_conflict_bar(None);
        self.set_pending_conflict(None);
        self.root_view().setNeedsLayout(true);
    }

    /// `dismissChangeSummary()`.
    pub fn dismiss_change_summary(&self) {
        if let Some(item) = self.change_summary_dismiss_work_item() {
            item.cancel();
        }
        self.set_change_summary_dismiss_work_item(None);
        if let Some(bar) = self.change_summary_bar() {
            bar.removeFromSuperview();
        }
        self.set_change_summary_bar(None);
        self.root_view().setNeedsLayout(true);
    }

    fn schedule_change_summary_dismissal(&self) {
        let weak = ObjcWeak::new(self);
        let work = WorkItem::new(move || {
            if let Some(this) = weak.load() {
                this.animate_change_summary_out();
            }
        });
        self.set_change_summary_dismiss_work_item(Some(work.clone()));
        work.dispatch_main_after(3.5);
    }

    fn animate_change_summary_in_if_needed(&self) {
        let Some(bar) = self.change_summary_bar() else { return };
        if let Some(layer) = bar.layer() {
            layer.removeAllAnimations();
        }
        let layer = bar.layer();
        let Some(layer) = layer.filter(|_| !self.active_style_sheet().reduce_motion) else {
            bar.setAlphaValue(1.0);
            return;
        };

        let transform = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        set_keyframe_values(
            &transform,
            &[
                transform_value(CATransform3D::new_scale(0.72, 0.82, 1.0)),
                transform_value(CATransform3D::new_scale(1.04, 0.96, 1.0)),
                transform_value(unsafe { CATransform3DIdentity }),
            ],
        );
        transform.setKeyTimes(Some(&numbers(&[NSNumber::new_isize(0), NSNumber::new_f64(0.64), NSNumber::new_isize(1)])));
        transform.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
            named_timing(unsafe { kCAMediaTimingFunctionEaseOut }),
            named_timing(unsafe { kCAMediaTimingFunctionEaseInEaseOut }),
        ])));
        transform.setDuration(0.42);

        let opacity = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        unsafe {
            opacity.setFromValue(Some(&NSNumber::new_isize(0)));
            opacity.setToValue(Some(&NSNumber::new_isize(1)));
        }
        opacity.setDuration(0.20);
        layer.addAnimation_forKey(&transform, Some(&NSString::from_str("change-toast-materialize")));
        layer.addAnimation_forKey(&opacity, Some(&NSString::from_str("change-toast-fade-in")));
        bar.setAlphaValue(1.0);
    }

    fn animate_change_summary_out(&self) {
        let Some(bar) = self.change_summary_bar() else { return };
        self.set_change_summary_dismiss_work_item(None);
        let layer = bar.layer();
        let Some(layer) = layer.filter(|_| !self.active_style_sheet().reduce_motion) else {
            let animated = bar.clone();
            let changes = RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
                let context = unsafe { context.as_ref() };
                context.setDuration(0.16);
                let animator = animated.animator();
                animator.setAlphaValue(0.0);
            });
            let weak = ObjcWeak::new(self);
            let completion = RcBlock::new(move || {
                let weak = weak.clone();
                main_async(move || {
                    if let Some(this) = weak.load() {
                        this.dismiss_change_summary();
                    }
                });
            });
            NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&completion));
            return;
        };

        let group = CAAnimationGroup::new();
        let transform = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        set_keyframe_values(
            &transform,
            &[
                transform_value(unsafe { CATransform3DIdentity }),
                transform_value(CATransform3D::new_scale(1.03, 0.94, 1.0)),
                transform_value(CATransform3D::new_scale(0.62, 0.74, 1.0)),
            ],
        );
        transform.setKeyTimes(Some(&numbers(&[NSNumber::new_isize(0), NSNumber::new_f64(0.30), NSNumber::new_isize(1)])));
        let opacity = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        unsafe {
            opacity.setFromValue(Some(&NSNumber::new_isize(1)));
            opacity.setToValue(Some(&NSNumber::new_isize(0)));
        }
        group.setAnimations(Some(&NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(transform)),
            Retained::into_super(Retained::into_super(opacity)),
        ])));
        group.setDuration(0.34);
        group.setTimingFunction(Some(&named_timing(unsafe { kCAMediaTimingFunctionEaseInEaseOut })));
        group.setRemovedOnCompletion(false);
        group.setFillMode(unsafe { kCAFillModeForwards });
        CATransaction::begin();
        let weak = ObjcWeak::new(self);
        let completion = RcBlock::new(move || {
            if let Some(this) = weak.load() {
                this.dismiss_change_summary();
            }
        });
        unsafe { CATransaction::setCompletionBlock(Some(&completion)) };
        layer.addAnimation_forKey(&group, Some(&NSString::from_str("change-toast-dematerialize")));
        CATransaction::commit();
    }
}

// MARK: - Helpers

/// `text.split(whereSeparator: { $0.isWhitespace }).count`: runs of
/// non-whitespace Characters.
fn whitespace_separated_count(text: &str) -> isize {
    let mut count = 0;
    let mut in_word = false;
    for grapheme in upleft_swift_text::graphemes(text) {
        if upleft_swift_text::is_whitespace(grapheme) {
            in_word = false;
        } else if !in_word {
            in_word = true;
            count += 1;
        }
    }
    count
}

/// `NSValue(caTransform3D:)`.
pub(super) fn transform_value(transform: CATransform3D) -> Retained<AnyObject> {
    // SAFETY: a plain value conversion.
    let value: Retained<NSValue> = unsafe { NSValue::valueWithCATransform3D(transform) };
    Retained::into_super(Retained::into_super(value))
}

pub(super) fn numbers(values: &[Retained<NSNumber>]) -> Retained<NSArray<NSNumber>> {
    NSArray::from_retained_slice(values)
}

pub(super) fn set_keyframe_values(animation: &CAKeyframeAnimation, values: &[Retained<AnyObject>]) {
    let array: Retained<NSArray> = NSArray::from_retained_slice(values);
    // SAFETY: every element is an `NSValue`/`NSNumber` of the key path's type.
    unsafe { animation.setValues(Some(&array)) };
}

/// `CAMediaTimingFunction(name:)`.
pub(super) fn named_timing(name: &objc2_quartz_core::CAMediaTimingFunctionName) -> Retained<CAMediaTimingFunction> {
    CAMediaTimingFunction::functionWithName(name)
}

#[allow(unused_imports)]
use {CAAnimation as _, kCAMediaTimingFunctionEaseIn as _};
