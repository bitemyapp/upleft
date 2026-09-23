//! Port of `Panels/UpdateStatusPill.swift`: a compact, themed status control
//! for the update flow. One instance lives in every document titlebar and
//! one in the start window; each observes the coordinator and renders the
//! same `pillModel`, so all surfaces agree.
//!
//! It remains absent (1pt wide, hidden) when the coordinator is idle, and
//! appears with a restrained fade/slide for the states the spec surfaces:
//! "Update Now", a progress ring, "Restart to Update", and a warning/retry
//! badge. All motion honors Reduce Motion.
//!
//! The actionable pill *acts*: one press installs and relaunches, rather
//! than opening a window to ask the question the label already answered.
//! What a press costs is therefore owed to the reader before they make it,
//! which is what `UpdateNotesPopover` is for — resting on the pill unfurls
//! the release notes without committing to anything.
//!
//! One consequence governs the whole control: **the pill must never move or
//! reshape under the pointer.** It is a button that restarts the app, so the
//! target cannot shift while it is being aimed at. The hover notes are a
//! separate window hanging below it, and the arrival emphasis is a colour
//! pass with no geometry in it at all.
//!
//! The pill observes `stateDidChange` from any object (`object: nil`, as in
//! Swift) and always reads `UpdateCoordinator::shared`.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAttributedStringNSStringDrawing, NSButton, NSButtonType, NSColor,
    NSControl, NSControlSize, NSEvent, NSFocusRingType, NSImageView, NSLayoutConstraint, NSLineBreakMode,
    NSProgressIndicator, NSProgressIndicatorStyle, NSResponder, NSTextField, NSTrackingArea, NSTrackingAreaOptions,
    NSView, NSWindowOcclusionState,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSNumber, NSOperationQueue, NSRect, NSSize, NSString};
use objc2_quartz_core::{CAKeyframeAnimation, CALayer, CABasicAnimation, CAMediaTiming, kCACornerCurveContinuous};
use upleft_render::core_types::CalloutKind;
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::style_sheet_defaults::PanelAlpha;

use super::appkit_support::{
    Presentation as LayerPresentation, RectExt, WorkItem, activate, cg, label, main_after, needs_display, ns_string,
    object, role,
    set_help, set_label, set_role, set_value, smax, system_symbol, symbol_configuration, weight_medium,
    weight_semibold, without_actions,
};
use super::panel_chrome::{PanelFont, refresh_tracking_area, set_number_values};
use super::update_notes_popover::UpdateNotesPopover;
use crate::app::toolbar_controls::{InteractionState, ToolbarChromePolicy};
use crate::updater::update_coordinator::{UpdateCoordinator, UpdatePillModel};

/// `UpdateStatusPill.Presentation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    Standard,
    /// The start window already has a strong task hierarchy. An update
    /// failure stays actionable, but collapses to a quiet warning button
    /// instead of competing with Open/New.
    CompactWarning,
}

/// `UpdateStatusPill.Metrics`.
struct Metrics;

impl Metrics {
    const HEIGHT: CGFloat = 26.0;
    const CORNER_RADIUS: CGFloat = 13.0;
    const HORIZONTAL_PADDING: CGFloat = 11.0;
    const ICON_SIZE: CGFloat = 13.0;
    const COMPACT_WARNING_WIDTH: CGFloat = 34.0;
}

pub struct UpdateStatusPillIvars {
    presentation: Presentation,
    shell: Retained<NSView>,
    /// The same hover/press wash every neighbouring toolbar control uses, so
    /// the pill answers the pointer instead of sitting inert among controls
    /// that do (§11.3).
    feedback_layer: Retained<CALayer>,
    interaction: Cell<InteractionState>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    icon_view: Retained<NSImageView>,
    label: Retained<NSTextField>,
    progress_indicator: Retained<NSProgressIndicator>,
    width_constraint: RefCell<Option<Retained<NSLayoutConstraint>>>,
    state_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    sheet: RefCell<Rc<StyleSheet>>,
    current_model: RefCell<Option<UpdatePillModel>>,
    /// A single accent pass when an update lands while the reader is here.
    /// Separate from `feedback_layer` so an emphasis in flight cannot be
    /// cancelled by a hover, and a hover cannot inherit the emphasis colour.
    emphasis_layer: Retained<CALayer>,
    hover_notes_work: RefCell<Option<WorkItem>>,
    /// Strong: the panel takes itself down on pointer-exit, so it has to
    /// survive the method that opened it.
    notes_popover: RefCell<Option<Rc<UpdateNotesPopover>>>,
}

impl Drop for UpdateStatusPillIvars {
    /// `deinit`.
    fn drop(&mut self) {
        if let Some(observer) = self.state_observer.get_mut().take() {
            // SAFETY: the token came from `addObserverForName:…`.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(object(&*observer)) };
        }
    }
}

define_class!(
    /// `UpdateStatusPill`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateStatusPill"]
    #[ivars = UpdateStatusPillIvars]
    pub struct UpdateStatusPill;

    unsafe impl NSObjectProtocol for UpdateStatusPill {}

    impl UpdateStatusPill {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            self.intrinsic_content_size()
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            *self.ivars().sheet.borrow_mut() = Self::make_sheet(self.mtm());
            self.refresh_appearance();
        }

        #[unsafe(method(clicked:))]
        fn __clicked(&self, _sender: Option<&AnyObject>) {
            // The notes were the preview; the click is the decision. Take the
            // panel down first so the app does not relaunch out from under a
            // window still animating.
            self.dismiss_notes_popover(false);
            UpdateCoordinator::shared(self.mtm()).user_did_press_update_now();
        }

        // MARK: - Pointer feedback

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            without_actions(|| {
                ivars.feedback_layer.setFrame(ivars.shell.bounds());
                ivars.emphasis_layer.setFrame(ivars.shell.bounds());
            });
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveInActiveApp
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.set_interaction(InteractionState::Hover);
            self.schedule_notes_popover();
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.set_interaction(InteractionState::Idle);
            // Only the *pending* presentation is cancelled here. A panel
            // already on screen dismisses itself once the pointer has left
            // the pill, the bridge, and its own body — that union is what
            // makes it enterable.
            self.cancel_hover_notes_work();
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.set_interaction(InteractionState::Pressed);
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            self.set_interaction(if self.bounds().contains_point(point) {
                InteractionState::Hover
            } else {
                InteractionState::Idle
            });
        }
    }
);

impl UpdateStatusPill {
    /// `init(presentation:)`; Swift's default is `.standard`.
    pub fn new(presentation: Presentation, mtm: MainThreadMarker) -> Retained<UpdateStatusPill> {
        // Stored-property initialisers first, in declaration order.
        let shell = NSView::new(mtm);
        let feedback_layer = CALayer::new();
        let icon_view = NSImageView::new(mtm);
        let label = label("", mtm);
        let progress_indicator = NSProgressIndicator::new(mtm);
        let emphasis_layer = CALayer::new();
        let sheet = Self::make_sheet(mtm);
        let this = Self::alloc(mtm).set_ivars(UpdateStatusPillIvars {
            presentation,
            shell: shell.clone(),
            feedback_layer: feedback_layer.clone(),
            interaction: Cell::new(InteractionState::Idle),
            tracking_area: RefCell::new(None),
            icon_view: icon_view.clone(),
            label: label.clone(),
            progress_indicator: progress_indicator.clone(),
            width_constraint: RefCell::new(None),
            state_observer: RefCell::new(None),
            sheet: RefCell::new(sheet),
            current_model: RefCell::new(None),
            emphasis_layer: emphasis_layer.clone(),
            hover_notes_work: RefCell::new(None),
            notes_popover: RefCell::new(None),
        });
        let this: Retained<UpdateStatusPill> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);

        shell.setTranslatesAutoresizingMaskIntoConstraints(false);
        shell.setWantsLayer(true);
        if let Some(layer) = shell.layer() {
            layer.setCornerRadius(Metrics::CORNER_RADIUS);
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        }
        feedback_layer.setCornerRadius(Metrics::CORNER_RADIUS);
        feedback_layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        feedback_layer.setOpacity(0.0);
        if let Some(layer) = shell.layer() {
            layer.addSublayer(&feedback_layer);
        }
        emphasis_layer.setCornerRadius(Metrics::CORNER_RADIUS);
        emphasis_layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        emphasis_layer.setOpacity(0.0);
        if let Some(layer) = shell.layer() {
            layer.addSublayer(&emphasis_layer);
        }
        this.addSubview(&shell);

        icon_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon_view.setSymbolConfiguration(Some(&symbol_configuration(Metrics::ICON_SIZE, weight_medium())));
        icon_view.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
        shell.addSubview(&icon_view);

        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.setFont(Some(&PanelFont::system(11.5, weight_semibold())));
        label.setMaximumNumberOfLines(1);
        label.setUsesSingleLineMode(true);
        label.setEditable(false);
        label.setSelectable(false);
        label.setBezeled(false);
        label.setDrawsBackground(false);
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        shell.addSubview(&label);

        progress_indicator.setTranslatesAutoresizingMaskIntoConstraints(false);
        progress_indicator.setStyle(NSProgressIndicatorStyle::Spinning);
        progress_indicator.setControlSize(NSControlSize::Small);
        progress_indicator.setDisplayedWhenStopped(false);
        shell.addSubview(&progress_indicator);

        this.setBordered(false);
        // The pill owns its content through `shell` and `label`; leaving the
        // NSButton title in place draws a second, colliding string underneath.
        this.setTitle(&ns_string(""));
        this.setButtonType(NSButtonType::MomentaryChange);
        this.setFocusRingType(NSFocusRingType::Default);
        unsafe {
            this.setTarget(Some(object(&*this)));
            this.setAction(Some(sel!(clicked:)));
        }
        set_role(&*this, role::button());
        set_help(&*this, "Upleft update status");
        // Pointer users get the same sentence VoiceOver does.
        this.setToolTip(Some(&ns_string("Install the update and relaunch Upleft")));

        activate(&[
            shell.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            shell.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            shell.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            shell.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            icon_view.leadingAnchor().constraintEqualToAnchor_constant(&shell.leadingAnchor(), Metrics::HORIZONTAL_PADDING),
            icon_view.centerYAnchor().constraintEqualToAnchor(&shell.centerYAnchor()),
            icon_view.widthAnchor().constraintEqualToConstant(Metrics::ICON_SIZE),
            icon_view.heightAnchor().constraintEqualToConstant(Metrics::ICON_SIZE),
            label.leadingAnchor().constraintEqualToAnchor_constant(&icon_view.trailingAnchor(), 5.0),
            label.centerYAnchor().constraintEqualToAnchor(&shell.centerYAnchor()),
            label.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(
                &shell.trailingAnchor(),
                -Metrics::HORIZONTAL_PADDING,
            ),
            progress_indicator.leadingAnchor().constraintEqualToAnchor(&icon_view.leadingAnchor()),
            progress_indicator.centerYAnchor().constraintEqualToAnchor(&shell.centerYAnchor()),
            progress_indicator.widthAnchor().constraintEqualToConstant(Metrics::ICON_SIZE + 2.0),
            progress_indicator.heightAnchor().constraintEqualToConstant(Metrics::ICON_SIZE + 2.0),
        ]);

        let width_constraint = this.widthAnchor().constraintEqualToConstant(0.0);
        *this.ivars().width_constraint.borrow_mut() = Some(width_constraint.clone());
        width_constraint.setActive(true);
        this.heightAnchor().constraintEqualToConstant(Metrics::HEIGHT).setActive(true);
        this.set_hidden(true, false);

        let weak: ObjcWeak<UpdateStatusPill> = ObjcWeak::from(&*this);
        let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
            if let Some(this) = weak.load() {
                this.refresh_from_coordinator();
            }
        });
        // SAFETY: the block runs on the main queue, where the weak reference
        // is loaded.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str(UpdateCoordinator::STATE_DID_CHANGE)),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *this.ivars().state_observer.borrow_mut() = Some(observer);
        this.refresh_from_coordinator();

        let _: () = unsafe { msg_send![&*this, viewDidChangeEffectiveAppearance] };
        this
    }

    /// `UpdateStatusPill()`.
    pub fn new_standard(mtm: MainThreadMarker) -> Retained<UpdateStatusPill> {
        Self::new(Presentation::Standard, mtm)
    }

    fn make_sheet(mtm: MainThreadMarker) -> Rc<StyleSheet> {
        Rc::new(StyleSheet::new(
            ThemeStore::shared().current(),
            &NSApplication::sharedApplication(mtm).effectiveAppearance(),
            None,
        ))
    }

    fn width_constraint(&self) -> Retained<NSLayoutConstraint> {
        self.ivars().width_constraint.borrow().clone().expect("set in init")
    }

    fn current_model(&self) -> Option<UpdatePillModel> {
        self.ivars().current_model.borrow().clone()
    }

    fn sheet(&self) -> Rc<StyleSheet> {
        self.ivars().sheet.borrow().clone()
    }

    /// `interaction`'s `didSet`.
    fn set_interaction(&self, value: InteractionState) {
        let old_value = self.ivars().interaction.replace(value);
        if value == old_value {
            return;
        }
        self.apply_feedback();
    }

    fn intrinsic_content_size(&self) -> NSSize {
        let ivars = self.ivars();
        let Some(current_model) = self.current_model() else {
            // Absent-but-in-the-toolbar: a zero width makes AppKit log an
            // ambiguous-layout warning on every launch. 1 is invisible yet
            // unambiguous; the pill's own width constraint collapses it.
            return NSSize::new(1.0, Metrics::HEIGHT);
        };
        if ivars.presentation == Presentation::CompactWarning && current_model == UpdatePillModel::Warning {
            return NSSize::new(Metrics::COMPACT_WARNING_WIDTH, Metrics::HEIGHT);
        }
        // NSTextField's attributed-string measurement can under-report a
        // fallback glyph such as the warning copy at small sizes.
        // fittingSize reflects the actual cell layout; retain the explicit
        // measurement as a lower-bound guard for custom fonts/themes.
        let text_width =
            smax(ivars.label.fittingSize().width, ivars.label.attributedStringValue().size().width).ceil() + 2.0;
        let icon_width: CGFloat = match current_model {
            UpdatePillModel::Progress(..) => Metrics::ICON_SIZE + 3.0,
            _ => Metrics::ICON_SIZE + 3.0,
        };
        NSSize::new(Metrics::HORIZONTAL_PADDING * 2.0 + icon_width + text_width, Metrics::HEIGHT)
    }

    // MARK: - Hover notes

    fn cancel_hover_notes_work(&self) {
        let work = self.ivars().hover_notes_work.borrow_mut().take();
        if let Some(work) = work {
            work.cancel();
        }
    }

    fn schedule_notes_popover(&self) {
        self.cancel_hover_notes_work();
        let Some(current_model) = self.current_model() else { return };
        if !current_model.offers_install() {
            return;
        }
        if UpdateCoordinator::shared(self.mtm()).pending_update().is_none() || self.window().is_none() {
            return;
        }
        let weak: ObjcWeak<UpdateStatusPill> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            if let Some(this) = weak.load() {
                this.present_notes_popover();
            }
        });
        *self.ivars().hover_notes_work.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(UpdateNotesPopover::HOVER_IN_DELAY);
    }

    fn present_notes_popover(&self) {
        *self.ivars().hover_notes_work.borrow_mut() = None;
        if self.ivars().interaction.get() == InteractionState::Idle {
            return;
        }
        let Some(current_model) = self.current_model() else { return };
        if !current_model.offers_install() {
            return;
        }
        let is_ready = match &current_model {
            UpdatePillModel::UpdateNow { is_ready, .. } => *is_ready,
            _ => true,
        };
        let popover = UpdateNotesPopover::present(
            self,
            UpdateCoordinator::shared(self.mtm()).pending_update(),
            is_ready,
            self.sheet(),
        );
        *self.ivars().notes_popover.borrow_mut() = popover.clone();
        let Some(popover) = popover else { return };
        // The tooltip and the panel answer the same question, and the system
        // would float the tooltip over the panel a second later. The panel
        // is the better answer, so it takes the tooltip's place while it is
        // up.
        self.setToolTip(None);
        let weak: ObjcWeak<UpdateStatusPill> = ObjcWeak::from(self);
        popover.set_on_dismiss(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                *this.ivars().notes_popover.borrow_mut() = None;
            }
            if let Some(this) = weak.load() {
                this.refresh_appearance();
            }
        })));
    }

    fn dismiss_notes_popover(&self, animated: bool) {
        self.cancel_hover_notes_work();
        let popover = self.ivars().notes_popover.borrow().clone();
        if let Some(popover) = popover {
            popover.dismiss(animated);
        }
        *self.ivars().notes_popover.borrow_mut() = None;
    }

    // MARK: - Arrival

    /// One accent pass, once, when an update lands while the reader is
    /// actually here. A pill that was already offering an update when the
    /// window opened has not just *arrived* and must not pretend it did.
    fn play_arrival_emphasis(&self) {
        let sheet = self.sheet();
        if sheet.reduce_motion {
            return;
        }
        let emphasis_layer = &self.ivars().emphasis_layer;
        emphasis_layer.setBackgroundColor(Some(&cg(&sheet.accent)));
        let pulse = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let values = NSArray::from_retained_slice(&[
            NSNumber::new_f64(0.0),
            NSNumber::new_f64(if sheet.increase_contrast { 0.5 } else { 0.34 }),
            NSNumber::new_f64(0.0),
        ]);
        unsafe { pulse.setValues(Some(Retained::cast_unchecked::<NSArray>(values).as_ref())) };
        let key_times =
            NSArray::from_retained_slice(&[NSNumber::new_isize(0), NSNumber::new_f64(0.34), NSNumber::new_isize(1)]);
        pulse.setKeyTimes(Some(&key_times));
        pulse.setDuration(motion::EMPHASIS * 4.0);
        let timing_functions = NSArray::from_retained_slice(&[motion::timing(Curve::Snap), motion::timing(Curve::EaseOut)]);
        pulse.setTimingFunctions(Some(&timing_functions));
        emphasis_layer.addAnimation_forKey(&pulse, Some(&NSString::from_str("update-arrival")));
    }

    fn apply_feedback(&self) {
        let ivars = self.ivars();
        let sheet = self.sheet();
        let feedback_layer = &ivars.feedback_layer;
        feedback_layer.setBackgroundColor(Some(&cg(&sheet.text)));
        let interaction = ivars.interaction.get();
        let opacity = ToolbarChromePolicy::feedback_opacity(interaction, sheet.increase_contrast);
        if sheet.reduce_motion || self.window().is_none() {
            without_actions(|| feedback_layer.setOpacity(opacity));
            return;
        }
        let fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let from = feedback_layer
            .__presentation()
            .map(|presentation| presentation.opacity())
            .unwrap_or_else(|| feedback_layer.opacity());
        set_number_values(&fade, Some(from as f64), opacity as f64);
        fade.setDuration(if interaction == InteractionState::Pressed {
            ToolbarChromePolicy::PRESS_IN_DURATION
        } else {
            ToolbarChromePolicy::HOVER_DURATION
        });
        fade.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        feedback_layer.addAnimation_forKey(&fade, Some(&NSString::from_str("feedback")));
        feedback_layer.setOpacity(opacity);
    }

    // MARK: - Coordinator binding

    fn refresh_from_coordinator(&self) {
        let mtm = self.mtm();
        let model = UpdateCoordinator::shared(mtm).pill_model();
        let current_model = self.current_model();
        let changed = model != current_model;
        let was_visible = current_model != UpdatePillModel::HIDDEN;
        *self.ivars().current_model.borrow_mut() = model.clone();
        self.refresh_appearance();
        if !changed {
            return;
        }
        let visible = model != UpdatePillModel::HIDDEN;
        if !model.as_ref().is_some_and(UpdatePillModel::offers_install) {
            self.dismiss_notes_popover(true);
        }
        // "Landed while you were sitting here" is the only case that earns
        // emphasis. A pill built for a window that is opening now, or one
        // whose app is in the background, has not interrupted anybody — and
        // the window check is what tells those apart, because a pill built
        // before it is in a window has no window to have interrupted.
        if visible
            && !was_visible
            && NSApplication::sharedApplication(mtm).isActive()
            && let Some(window) = self.window()
            && window.isVisible()
            && window.occlusionState().contains(NSWindowOcclusionState::Visible)
        {
            self.play_arrival_emphasis();
        }
        // Set in both branches: a hidden control that still reports the last
        // visible state is a stale answer for VoiceOver.
        set_label(self, &Self::accessibility_label_for(model.as_ref()));
        let value = if visible { self.ivars().label.stringValue().to_string() } else { String::new() };
        set_value(self, &value);
        if visible {
            self.setHidden(false);
        }
        let reduce = self.sheet().reduce_motion;
        // Collapse to 1pt, never 0: the toolbar auto-measures this view and
        // a zero-width frame logs an ambiguous-layout warning on every launch.
        let target_width: CGFloat = if visible { self.intrinsic_content_size().width } else { 1.0 };
        let shell = &self.ivars().shell;
        if reduce {
            self.width_constraint().setConstant(target_width);
            if !visible {
                self.setHidden(true);
            }
            shell.setAlphaValue(if visible { 1.0 } else { 0.0 });
            self.layoutSubtreeIfNeeded();
        } else {
            let width_animator: Retained<NSLayoutConstraint> = unsafe { msg_send![&*self.width_constraint(), animator] };
            width_animator.setConstant(target_width);
            let shell_animator: Retained<NSView> = unsafe { msg_send![&**shell, animator] };
            shell_animator.setAlphaValue(if visible { 1.0 } else { 0.0 });
            if !visible {
                let weak: ObjcWeak<UpdateStatusPill> = ObjcWeak::from(self);
                main_after(motion::STANDARD, move || {
                    let Some(this) = weak.load() else { return };
                    if this.current_model() != UpdatePillModel::HIDDEN {
                        return;
                    }
                    this.setHidden(true);
                });
            }
        }
    }

    fn refresh_appearance(&self) {
        let Some(model) = self.current_model() else { return };
        let ivars = self.ivars();
        let sheet = self.sheet();
        let contrast = sheet.increase_contrast;
        let icon_view = &ivars.icon_view;
        let label = &ivars.label;
        let progress_indicator = &ivars.progress_indicator;
        let shell_layer = ivars.shell.layer();
        let accent_fill = || {
            sheet
                .surface
                .blendedColorWithFraction_ofColor(if contrast { 0.5 } else { 0.3 }, &sheet.accent)
                .map(|color| cg(&color))
                .unwrap_or_else(|| cg(&sheet.surface))
        };
        match &model {
            UpdatePillModel::UpdateNow { version, is_ready } => {
                icon_view.setImage(system_symbol("arrow.down.circle.fill", Some("Update available")).as_deref());
                icon_view.setContentTintColor(Some(&sheet.accent));
                // The button says what pressing it does; the version it
                // installs is one hover away, and in the tooltip for pointer
                // users who never rest long enough to see the notes.
                label.setTextColor(Some(&sheet.text));
                label.setStringValue(&ns_string("Update Now"));
                label.setHidden(false);
                let tool_tip = if version.is_empty() {
                    "Install the update and relaunch Upleft".to_owned()
                } else if *is_ready {
                    format!("Install Upleft {version} and relaunch")
                } else {
                    format!("Download and install Upleft {version}, then relaunch")
                };
                self.setToolTip(Some(&ns_string(&tool_tip)));
                unsafe { progress_indicator.stopAnimation(None) };
                progress_indicator.setHidden(true);
                icon_view.setHidden(false);
                if let Some(layer) = &shell_layer {
                    layer.setBackgroundColor(Some(&accent_fill()));
                    layer.setBorderWidth(0.0);
                }
            }
            UpdatePillModel::RestartToUpdate => {
                icon_view.setImage(system_symbol("arrow.clockwise.circle.fill", Some("Restart to update")).as_deref());
                icon_view.setContentTintColor(Some(&sheet.accent));
                label.setTextColor(Some(&sheet.text));
                label.setStringValue(&ns_string("Restart to Update"));
                label.setHidden(false);
                self.setToolTip(Some(&ns_string("Quit and reopen Upleft to finish installing")));
                unsafe { progress_indicator.stopAnimation(None) };
                progress_indicator.setHidden(true);
                icon_view.setHidden(false);
                if let Some(layer) = &shell_layer {
                    layer.setBackgroundColor(Some(&accent_fill()));
                    layer.setBorderWidth(0.0);
                }
            }
            UpdatePillModel::Progress(title, fraction) => {
                label.setTextColor(Some(&sheet.text_secondary));
                label.setStringValue(&ns_string(title));
                label.setHidden(false);
                self.setToolTip(Some(&ns_string("Show update progress")));
                icon_view.setHidden(true);
                progress_indicator.setHidden(false);
                if let Some(fraction) = fraction {
                    progress_indicator.setStyle(NSProgressIndicatorStyle::Bar);
                    progress_indicator.setDoubleValue(fraction * 100.0);
                } else {
                    progress_indicator.setStyle(NSProgressIndicatorStyle::Spinning);
                    unsafe { progress_indicator.startAnimation(None) };
                }
                if let Some(layer) = &shell_layer {
                    layer.setBackgroundColor(Some(&cg(&sheet.surface)));
                    layer.setBorderWidth(if contrast { 1.0 } else { 0.0 });
                    layer.setBorderColor(Some(&cg(&sheet.rule)));
                }
            }
            UpdatePillModel::Warning => {
                icon_view.setImage(system_symbol("exclamationmark.triangle.fill", Some("Update failed")).as_deref());
                // The theme owns the warning colour; a raw system orange
                // ignores the warm light and dark palettes entirely (§11.3).
                let warning = sheet.callout_color(CalloutKind::Warning);
                let compact = ivars.presentation == Presentation::CompactWarning;
                icon_view.setContentTintColor(Some(&warning));
                label.setTextColor(Some(&warning));
                label.setStringValue(&ns_string("Update Failed"));
                label.setHidden(compact);
                self.setToolTip(Some(&ns_string(if compact {
                    "Update failed — click for details"
                } else {
                    "Open the update panel"
                })));
                unsafe { progress_indicator.stopAnimation(None) };
                progress_indicator.setHidden(true);
                icon_view.setHidden(false);
                if let Some(layer) = &shell_layer {
                    if compact {
                        layer.setBackgroundColor(Some(&cg(&warning.panel_alpha(if contrast { 0.12 } else { 0.07 }, false))));
                        layer.setBorderWidth(1.0);
                        layer.setBorderColor(Some(&cg(&warning.colorWithAlphaComponent(if contrast { 0.55 } else { 0.32 }))));
                    } else {
                        layer.setBackgroundColor(Some(&cg(&warning.panel_alpha(if contrast { 0.22 } else { 0.12 }, false))));
                        layer.setBorderWidth(if contrast { 1.0 } else { 0.0 });
                        layer.setBorderColor(Some(&cg(&warning)));
                    }
                }
            }
            UpdatePillModel::Informational(version) => {
                icon_view.setImage(system_symbol("info.circle.fill", Some("Update information")).as_deref());
                self.setToolTip(Some(&ns_string(&format!("Read about Upleft {version}"))));
                icon_view.setContentTintColor(Some(&sheet.accent));
                label.setTextColor(Some(&sheet.text_secondary));
                label.setStringValue(&ns_string(&format!("Update {version}")));
                label.setHidden(false);
                unsafe { progress_indicator.stopAnimation(None) };
                progress_indicator.setHidden(true);
                icon_view.setHidden(false);
                if let Some(layer) = &shell_layer {
                    layer.setBackgroundColor(Some(&cg(&sheet.surface)));
                    layer.setBorderWidth(if contrast { 1.0 } else { 0.0 });
                    layer.setBorderColor(Some(&cg(&sheet.rule)));
                }
            }
        }
        self.apply_feedback();
        needs_display(self);
    }

    fn accessibility_label_for(model: Option<&UpdatePillModel>) -> String {
        match model {
            Some(UpdatePillModel::UpdateNow { version, is_ready }) => {
                if version.is_empty() {
                    return "Update Now".to_owned();
                }
                if *is_ready {
                    format!("Update Now. Upleft {version} is ready to install")
                } else {
                    format!("Update Now. Upleft {version} is available")
                }
            }
            Some(UpdatePillModel::RestartToUpdate) => "Update ready. Restart Upleft to update".to_owned(),
            Some(UpdatePillModel::Progress(title, _)) => title.clone(),
            Some(UpdatePillModel::Warning) => "Update failed. Open the update panel for details".to_owned(),
            Some(UpdatePillModel::Informational(version)) => format!("Update {version} information"),
            None => "No update status".to_owned(),
        }
    }

    /// `setHidden(_:animated:)`.
    fn set_hidden(&self, hidden: bool, _animated: bool) {
        self.setHidden(hidden);
        self.ivars().shell.setAlphaValue(if hidden { 0.0 } else { 1.0 });
        // 1pt rather than 0 — see `refresh_from_coordinator`.
        let constant = if hidden { 1.0 } else { self.intrinsic_content_size().width };
        self.width_constraint().setConstant(constant);
    }

    // MARK: - Test and scene access (private in Swift)

    /// `currentModel`.
    pub fn current_model_for_testing(&self) -> Option<UpdatePillModel> {
        self.current_model()
    }

    /// `presentation`.
    pub fn presentation(&self) -> Presentation {
        self.ivars().presentation
    }

    /// The label, icon and spinner subviews.
    pub fn label_for_testing(&self) -> Retained<NSTextField> {
        self.ivars().label.clone()
    }

    pub fn icon_view_for_testing(&self) -> Retained<NSImageView> {
        self.ivars().icon_view.clone()
    }

    pub fn progress_indicator_for_testing(&self) -> Retained<NSProgressIndicator> {
        self.ivars().progress_indicator.clone()
    }

    /// The width constraint's constant.
    pub fn width_constant_for_testing(&self) -> CGFloat {
        self.width_constraint().constant()
    }

    /// Whether the style sheet the pill drew with reduces motion.
    pub fn reduces_motion_for_testing(&self) -> bool {
        self.sheet().reduce_motion
    }
}
