//! Port of `Panels/VersionTimelineView.swift`: the version timeline scrubber
//! (§8.3, `⌘⇧V`).
//!
//! Ticks are positioned **by time, not by index**.  That is the whole
//! design: an agent that rewrote the file five times in one minute should
//! look like a burst, and the afternoon you spent editing it yourself should
//! look like a gap.  Evenly spaced ticks would erase exactly the information
//! you opened the timeline to find.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSBezierPath, NSButton, NSColor, NSEvent, NSLineBreakMode, NSMutableParagraphStyle, NSResponder,
    NSStringDrawing, NSStringDrawingOptions, NSStringNSExtendedStringDrawing, NSTrackingArea, NSTrackingAreaOptions,
    NSView, NSViewNoIntrinsicMetric,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use upleft_foundation::date::Date;
use upleft_render::appkit_compat::{attributes_dictionary, keys};
use upleft_render::core_types::ChangeKind;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;

use super::appkit_support::{
    RectExt, activate, ns_string, object, rect, rect_fill, role, set_label, set_role, set_tool_tip, smax, smin,
};
use super::panel_chrome::{ButtonAction, PanelButton, PanelFont, PanelMetrics, RelativeTime, element_at, refresh_tracking_area};
use crate::ai::snapshot_store::{SnapshotKind, VersionRecord};
use crate::support::commands::KeyBinding;

/// `VersionTimelineDelegate`.
pub trait VersionTimelineDelegate {
    fn version_timeline_did_scrub_to(&self, view: &VersionTimelineView, record: &VersionRecord);
    fn version_timeline_did_request_restore(&self, view: &VersionTimelineView, record: &VersionRecord);
}

pub struct VersionTimelineViewIvars {
    delegate: RefCell<Option<Weak<dyn VersionTimelineDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    versions: RefCell<Vec<VersionRecord>>,
    selected_index: Cell<isize>,
    restore_button: Retained<NSButton>,
    restore_action: RefCell<Option<Retained<ButtonAction>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    hovered_index: Cell<Option<isize>>,
}

define_class!(
    /// `VersionTimelineView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "VersionTimelineView"]
    #[ivars = VersionTimelineViewIvars]
    pub struct VersionTimelineView;

    unsafe impl NSObjectProtocol for VersionTimelineView {}

    impl VersionTimelineView {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            // SAFETY: AppKit exports the metric as an immutable global.
            NSSize::new(unsafe { NSViewNoIntrinsicMetric }, 84.0)
        }

        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            self.draw(dirty_rect);
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn __accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        /// The scrubber takes the keyboard, so it has to show that it has it.
        #[unsafe(method(focusRingMaskBounds))]
        fn __focus_ring_mask_bounds(&self) -> NSRect {
            self.track_rect().inset_by(-4.0, -Self::TICK_HEIGHT)
        }

        #[unsafe(method(drawFocusRingMask))]
        fn __draw_focus_ring_mask(&self) {
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                self.track_rect().inset_by(-4.0, -Self::TICK_HEIGHT),
                PanelMetrics::CORNER_RADIUS,
                PanelMetrics::CORNER_RADIUS,
            )
            .fill();
        }

        #[unsafe(method(becomeFirstResponder))]
        fn __become_first_responder(&self) -> bool {
            self.setNeedsDisplay(true);
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn __resign_first_responder(&self) -> bool {
            self.setNeedsDisplay(true);
            true
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.scrub(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            self.scrub(event);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseMoved:))]
        fn __mouse_moved(&self, event: &NSEvent) {
            self.mouse_moved(event);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.mouse_exited();
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            match KeyBinding::key_for_event(event).as_deref() {
                Some("left") => self.step(-1),
                Some("right") => self.step(1),
                _ => {
                    let _: () = unsafe { msg_send![super(self), keyDown: event] };
                }
            }
        }

        #[unsafe(method(accessibilityPerformIncrement))]
        fn __accessibility_perform_increment(&self) -> bool {
            self.step(1);
            true
        }

        #[unsafe(method(accessibilityPerformDecrement))]
        fn __accessibility_perform_decrement(&self) -> bool {
            self.step(-1);
            true
        }
    }
);

impl VersionTimelineView {
    const TRACK_HEIGHT: CGFloat = 3.0;
    const TICK_HEIGHT: CGFloat = 16.0;
    const HORIZONTAL_INSET: CGFloat = 16.0;
    const RESTORE_WIDTH: CGFloat = 84.0;

    // MARK: - Init

    /// `VersionTimelineView()`: hosts build panels before they have a theme
    /// in hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<VersionTimelineView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<VersionTimelineView> {
        let action = ButtonAction::noop(mtm);
        let restore_button = PanelButton::text("Restore", &action, false, mtm);
        let this = Self::alloc(mtm).set_ivars(VersionTimelineViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            versions: RefCell::new(Vec::new()),
            selected_index: Cell::new(0),
            restore_button: restore_button.clone(),
            restore_action: RefCell::new(None),
            tracking_area: RefCell::new(None),
            hovered_index: Cell::new(None),
        });
        let this: Retained<VersionTimelineView> =
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, 520.0, 84.0)] };
        // The placeholder target is only ever held weakly by the button, so
        // it goes away here, as the Swift local does.
        drop(action);

        let weak: ObjcWeak<VersionTimelineView> = ObjcWeak::from(&*this);
        let restore = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                let Some(record) = this.selected_record() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.version_timeline_did_request_restore(&this, &record);
                }
            },
            mtm,
        );
        *this.ivars().restore_action.borrow_mut() = Some(restore.clone());
        unsafe {
            restore_button.setTarget(Some(object(&*restore)));
            restore_button.setAction(Some(ButtonAction::selector()));
        }
        restore_button.setEnabled(false);
        this.addSubview(&restore_button);

        activate(&[
            restore_button
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&this.trailingAnchor(), -Self::HORIZONTAL_INSET),
            restore_button.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            restore_button.widthAnchor().constraintEqualToConstant(Self::RESTORE_WIDTH),
        ]);

        this.setAccessibilityElement(true);
        set_role(&*this, role::slider());
        set_label(&*this, "Version timeline");
        this.update_accessibility();
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn VersionTimelineDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn VersionTimelineDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn versions(&self) -> Vec<VersionRecord> {
        self.ivars().versions.borrow().clone()
    }

    pub fn set_versions(&self, versions: Vec<VersionRecord>) {
        let count = versions.len() as isize;
        *self.ivars().versions.borrow_mut() = versions;
        self.set_selected_index(0.max(self.selected_index()).min(0.max(count - 1)));
        self.ivars().restore_button.setEnabled(count != 0);
        self.setNeedsDisplay(true);
    }

    pub fn selected_index(&self) -> isize {
        self.ivars().selected_index.get()
    }

    pub fn set_selected_index(&self, index: isize) {
        let old = self.ivars().selected_index.replace(index);
        if index == old {
            return;
        }
        self.update_accessibility();
        self.setNeedsDisplay(true);
    }

    /// `selectedRecord`.
    pub fn selected_record(&self) -> Option<VersionRecord> {
        let index = self.selected_index();
        let versions = self.ivars().versions.borrow();
        if !(index >= 0 && index < versions.len() as isize) {
            return None;
        }
        Some(versions[index as usize].clone())
    }

    fn apply_style(&self) {
        self.update_accessibility();
        self.setNeedsDisplay(true);
    }

    fn update_accessibility(&self) {
        let Some(record) = self.selected_record() else {
            self.set_value_description("No versions");
            return;
        };
        self.set_value_description(&format!(
            "{}, {}, {}",
            Self::kind_label(record.kind),
            RelativeTime::stamp(record.date),
            RelativeTime::long(record.date, Date::now())
        ));
    }

    fn set_value_description(&self, text: &str) {
        let text = ns_string(text);
        let _: () = unsafe { msg_send![self, setAccessibilityValueDescription: &*text] };
    }

    // MARK: - Geometry

    fn track_rect(&self) -> NSRect {
        rect(
            Self::HORIZONTAL_INSET,
            26.0,
            smax(40.0, self.bounds().width() - Self::HORIZONTAL_INSET * 2.0 - Self::RESTORE_WIDTH - 12.0),
            Self::TRACK_HEIGHT,
        )
    }

    fn x_for(&self, date: Date) -> CGFloat {
        let track = self.track_rect();
        let (first, last) = {
            let versions = self.ivars().versions.borrow();
            match (versions.first(), versions.last()) {
                (Some(first), Some(last)) => (first.date, last.date),
                _ => return track.min_x(),
            }
        };
        let span = last.time_interval_since(first);
        // A history that is one burst has no meaningful time axis; centre it
        // rather than pile every tick on the left edge.
        if !(span > 1.0) {
            return track.mid_x();
        }
        let fraction = date.time_interval_since(first) / span;
        track.min_x() + smin(1.0, smax(0.0, fraction)) * track.width()
    }

    fn nearest_index(&self, position: CGFloat) -> Option<isize> {
        let dates: Vec<Date> = self.ivars().versions.borrow().iter().map(|record| record.date).collect();
        if dates.is_empty() {
            return None;
        }
        let mut best: isize = 0;
        let mut best_distance = CGFloat::MAX;
        for (index, date) in dates.into_iter().enumerate() {
            let distance = (self.x_for(date) - position).abs();
            if distance < best_distance {
                best_distance = distance;
                best = index as isize;
            }
        }
        Some(best)
    }

    fn color(&self, kind: SnapshotKind) -> Retained<NSColor> {
        let style_sheet = self.style_sheet();
        match kind {
            // The writes you did not make are the ones you came here for.
            SnapshotKind::External => style_sheet.change_color(ChangeKind::Modified),
            SnapshotKind::Local => style_sheet.text_secondary.clone(),
            SnapshotKind::Baseline => style_sheet.text_faint.clone(),
        }
    }

    /// `kindLabel(_:)`.
    pub fn kind_label(kind: SnapshotKind) -> &'static str {
        match kind {
            SnapshotKind::External => "External change",
            SnapshotKind::Local => "Local save",
            SnapshotKind::Baseline => "Baseline",
        }
    }

    // MARK: - Drawing

    fn draw(&self, _dirty_rect: NSRect) {
        let track = self.track_rect();
        let style_sheet = self.style_sheet();
        let contrast = style_sheet.increase_contrast;

        style_sheet.rule.setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, 1.5, 1.5).fill();

        let versions = self.versions();
        if versions.is_empty() {
            self.draw_caption("No saved versions yet", track.mid_x(), &style_sheet.text_faint);
            return;
        }

        let selected_index = self.selected_index();
        let hovered_index = self.ivars().hovered_index.get();
        for (index, record) in versions.iter().enumerate() {
            let index = index as isize;
            if index == selected_index {
                continue;
            }
            // Semi-transparent so a cluster of writes reads darker than a
            // lone one — the density *is* the signal.  The tick under the
            // pointer comes forward, so a burst is scrubbable rather than a
            // smudge.
            let hovered = Some(index) == hovered_index;
            self.color(record.kind).panel_alpha(if hovered { 0.95 } else { 0.55 }, contrast).setFill();
            let position = self.x_for(record.date);
            let height = if hovered { Self::TICK_HEIGHT + 4.0 } else { Self::TICK_HEIGHT };
            let tick = rect(
                position - (if hovered { 1.5 } else { 1.0 }),
                track.mid_y() - height / 2.0,
                if hovered { 3.0 } else { 2.0 },
                height,
            );
            match record.kind {
                SnapshotKind::External => {
                    let wedge = NSBezierPath::new();
                    wedge.moveToPoint(NSPoint::new(tick.mid_x(), tick.min_y() - 3.0));
                    wedge.lineToPoint(NSPoint::new(tick.min_x() - 2.0, tick.min_y() + 2.0));
                    wedge.lineToPoint(NSPoint::new(tick.max_x() + 2.0, tick.min_y() + 2.0));
                    wedge.closePath();
                    wedge.fill();
                    rect_fill(tick);
                }
                SnapshotKind::Local => rect_fill(tick),
                SnapshotKind::Baseline => {
                    NSBezierPath::bezierPathWithOvalInRect(rect(tick.mid_x() - 3.0, track.mid_y() - 3.0, 6.0, 6.0))
                        .fill();
                }
            }
        }

        let Some(record) = self.selected_record() else { return };
        let position = self.x_for(record.date);

        style_sheet.accent.setFill();
        rect_fill(rect(position - 1.5, track.mid_y() - Self::TICK_HEIGHT / 2.0 - 3.0, 3.0, Self::TICK_HEIGHT + 6.0));
        let knob = rect(position - 5.0, track.mid_y() - 5.0, 10.0, 10.0);
        NSBezierPath::bezierPathWithOvalInRect(knob).fill();

        self.draw_caption(
            &format!(
                "{}  ·  {}  ·  {}",
                Self::kind_label(record.kind),
                RelativeTime::stamp(record.date),
                RelativeTime::long(record.date, Date::now())
            ),
            position,
            &style_sheet.text,
        );
    }

    fn draw_caption(&self, text: &str, center_x: CGFloat, color: &NSColor) {
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        let font = PanelFont::secondary();
        let attributes = attributes_dictionary(&[
            (keys::font(), object(&*font)),
            (keys::foreground_color(), object(color)),
            (keys::paragraph_style(), object(&*paragraph)),
        ]);
        let string = ns_string(text);
        let size = unsafe { string.sizeWithAttributes(Some(&attributes)) };
        let track = self.track_rect();
        // The caption may not run under the Restore button: it lives inside
        // the track's span, centred on the knob where it fits, truncated
        // where it does not — the inspector's minimum width is exactly the
        // case that used to let it bleed across the button.
        let available = smax(40.0, track.width());
        let lower = track.min_x();
        let upper = smax(lower, track.max_x() - smin(size.width, available));
        let x = smin(smax(lower, center_x - size.width / 2.0), upper);
        unsafe {
            string.drawWithRect_options_attributes_context(
                rect(x, track.max_y() + 12.0, smin(size.width, available), 18.0),
                NSStringDrawingOptions::UsesLineFragmentOrigin,
                Some(&attributes),
                None,
            );
        }
    }

    // MARK: - Hover

    fn mouse_moved(&self, event: &NSEvent) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let index = if (point.y - self.track_rect().mid_y()).abs() <= Self::TICK_HEIGHT {
            self.nearest_index(point.x)
        } else {
            None
        };
        if index == self.ivars().hovered_index.get() {
            return;
        }
        self.ivars().hovered_index.set(index);
        // The tooltip names the version the pointer is over, which is the
        // one thing a tick cannot say by itself.
        let record = index.and_then(|index| element_at(&self.ivars().versions.borrow(), index).cloned());
        let tip = record.map(|record| {
            let kind = Self::kind_label(record.kind);
            if let Some(selected) = self.selected_record() {
                // Swift's `Int(_:)` truncates toward zero.
                let diff = record.date.time_interval_since(selected.date) as isize;
                let delta = if diff == 0 {
                    "selected".to_owned()
                } else if diff > 0 {
                    format!("+{diff}s")
                } else {
                    format!("{diff}s")
                };
                return format!(
                    "{kind} ({delta}) · {} · {}",
                    RelativeTime::stamp(record.date),
                    RelativeTime::long(record.date, Date::now())
                );
            }
            format!("{kind} · {} · {}", RelativeTime::stamp(record.date), RelativeTime::long(record.date, Date::now()))
        });
        set_tool_tip(self, tip.as_deref());
        self.setNeedsDisplay(true);
    }

    fn mouse_exited(&self) {
        if self.ivars().hovered_index.get().is_none() {
            return;
        }
        self.ivars().hovered_index.set(None);
        set_tool_tip(self, None);
        self.setNeedsDisplay(true);
    }

    // MARK: - Scrubbing

    fn scrub(&self, event: &NSEvent) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let Some(index) = self.nearest_index(point.x) else { return };
        if index == self.selected_index() {
            return;
        }
        self.set_selected_index(index);
        if let Some(record) = self.selected_record()
            && let Some(delegate) = self.delegate()
        {
            delegate.version_timeline_did_scrub_to(self, &record);
        }
    }

    fn step(&self, delta: isize) {
        let count = self.ivars().versions.borrow().len() as isize;
        if count == 0 {
            return;
        }
        let next = 0.max(self.selected_index() + delta).min(count - 1);
        if next == self.selected_index() {
            return;
        }
        self.set_selected_index(next);
        if let Some(record) = self.selected_record()
            && let Some(delegate) = self.delegate()
        {
            delegate.version_timeline_did_scrub_to(self, &record);
        }
    }

    /// `accessibilityPerformIncrement()` / `…Decrement()`, which call
    /// `step(_:)` (the arrow keys do the same).
    pub fn step_for_testing(&self, delta: isize) {
        self.step(delta);
    }

    /// The Restore button (tests and the conformance scene read it).
    pub fn restore_button_for_testing(&self) -> Retained<NSButton> {
        self.ivars().restore_button.clone()
    }
}

#[allow(unused)]
fn _unused(_: &dyn NSAccessibility) {}
