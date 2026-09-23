//! Port of `Panels/ChangeSummaryBarView.swift`: the clean-buffer change
//! summary (§8.1).
//!
//! The document has already been updated in place underneath the reader —
//! this only reports what happened and offers to walk them, which is the
//! pointer equivalent of `[` and `]` (§7.2).  Same non-modal shape as the
//! conflict bar: it must be ignorable.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSButton, NSResponder, NSView};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSRect, NSSize};
use upleft_core::contracts::{ChangeKind, Uuid};
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{ns_string, set_label, smax, smin};
use super::panel_chrome::{MessageBarInit, MessageBarView, PanelMetrics};
use crate::ai::change_tracker::Mark;

/// `ChangeSummaryBarDelegate`.
pub trait ChangeSummaryBarDelegate {
    fn change_summary_bar_did_request_jump(&self, bar: &ChangeSummaryBarView, forward: bool);
    fn change_summary_bar_did_request_mark_reviewed(&self, bar: &ChangeSummaryBarView);
    fn change_summary_bar_did_request_dismiss(&self, bar: &ChangeSummaryBarView);
}

/// `ChangeSummaryBarView.Summary`: what a write did to the document, in the
/// terms a reader deciding whether to look would use.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    pub added: isize,
    pub rewritten: isize,
    pub removed: isize,
    /// Midpoint of each change as a fraction of the document, in document
    /// order — the shape of the write.
    pub positions: Vec<Position>,
}

/// `Summary.Position`.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub fraction: f64,
    pub kind: ChangeKind,
    /// The mark this position was derived from.
    pub id: Uuid,
}

impl Summary {
    pub fn total(&self) -> isize {
        self.added + self.rewritten + self.removed
    }

    /// `init(marks:documentLength:)`: derives the summary from the tracker's
    /// own marks. A zero or negative length omits the positions.
    pub fn from_marks(marks: &[Mark], document_length: isize) -> Summary {
        let mut summary = Summary::default();
        for mark in marks {
            match mark.kind {
                ChangeKind::Inserted => summary.added += 1,
                ChangeKind::Modified => summary.rewritten += 1,
                ChangeKind::Deleted => summary.removed += 1,
            }
        }
        if !(document_length > 0) {
            return summary;
        }
        let mut positions: Vec<Position> = marks
            .iter()
            .map(|mark| {
                let midpoint = mark.range.location as f64 + mark.range.length as f64 / 2.0;
                Position {
                    fraction: smin(1.0, smax(0.0, midpoint / document_length as f64)),
                    kind: mark.kind,
                    id: mark.id,
                }
            })
            .collect();
        // Swift's `sorted(by:)` is stable, as `sort_by` is.
        positions.sort_by(|a, b| {
            if a.fraction < b.fraction {
                std::cmp::Ordering::Less
            } else if b.fraction < a.fraction {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        summary.positions = positions;
        summary
    }

    /// `init(changeCount:)`: a summary carrying only a count, reported as
    /// rewrites.
    pub fn from_change_count(change_count: isize) -> Summary {
        Summary { rewritten: change_count.max(0), ..Summary::default() }
    }

    /// The sentence the bar leads with.
    pub fn headline(&self) -> String {
        let parts: Vec<(isize, &str)> =
            [(self.added, "added"), (self.rewritten, "rewritten"), (self.removed, "removed")]
                .into_iter()
                .filter(|part| part.0 > 0)
                .collect();
        let Some(first) = parts.first() else { return "Updated on disk".to_owned() };
        if parts.len() <= 1 {
            return format!("{} {} {}", first.0, if first.0 == 1 { "change" } else { "changes" }, first.1);
        }
        parts.iter().map(|part| format!("{} {}", part.0, part.1)).collect::<Vec<_>>().join(" · ")
    }

    /// Spoken form.
    pub fn accessibility_description(&self) -> String {
        if !(self.total() > 0) {
            return "Document updated on disk. No unread changes.".to_owned();
        }
        let mut sentence = format!("Document updated on disk. {}.", self.headline());
        if let Some(spread) = self.distribution_description() {
            sentence += &format!(" {spread}.");
        }
        sentence
    }

    /// Where the changes fall, in words; only claimed when the marks cluster.
    pub fn distribution_description(&self) -> Option<String> {
        if !(self.positions.len() > 1) {
            return None;
        }
        let low = self.positions.first()?.fraction;
        let high = self.positions.last()?.fraction;
        if !(high - low < 0.34) {
            return Some("Spread through the document".to_owned());
        }
        let middle = (low + high) / 2.0;
        if middle < 0.34 {
            return Some("Clustered near the start".to_owned());
        }
        if middle > 0.66 {
            return Some("Clustered near the end".to_owned());
        }
        Some("Clustered in the middle".to_owned())
    }
}

pub struct ChangeSummaryBarViewIvars {
    delegate: RefCell<Option<Weak<dyn ChangeSummaryBarDelegate>>>,
    summary: RefCell<Summary>,
    current_position: Cell<Option<isize>>,
    previous_button: RefCell<ObjcWeak<NSButton>>,
    next_button: RefCell<ObjcWeak<NSButton>>,
    reviewed_button: RefCell<ObjcWeak<NSButton>>,
}

define_class!(
    /// `ChangeSummaryBarView`, a `MessageBarView`.
    // SAFETY: `initWithFrame:` is forwarded to `MessageBarView` in `new`
    // after the ivars are set and the base's arguments are staged.
    #[unsafe(super(MessageBarView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ChangeSummaryBarView"]
    #[ivars = ChangeSummaryBarViewIvars]
    pub struct ChangeSummaryBarView;

    unsafe impl NSObjectProtocol for ChangeSummaryBarView {}

    impl ChangeSummaryBarView {
        /// Wide enough for the message it is carrying, never wider than a
        /// strip of chrome should be over a document.
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(
                smin(Self::MAXIMUM_WIDTH, smax(Self::MINIMUM_WIDTH, self.fitted_width())),
                Self::TOAST_HEIGHT,
            )
        }

        #[unsafe(method(applyStyle))]
        fn __apply_style(&self) {
            let style_sheet = self.style_sheet();
            self.set_stripe_color(style_sheet.change_color(ChangeKind::Inserted));
            let _: () = unsafe { msg_send![super(self), applyStyle] };
            // The tint ladder follows the action hierarchy: the walk chevrons
            // in secondary, dismiss (in the base class) one step fainter, and
            // the confirm key wearing the stripe's own green.
            let ivars = self.ivars();
            if let Some(button) = ivars.previous_button.borrow().load() {
                button.setContentTintColor(Some(&style_sheet.text_secondary));
            }
            if let Some(button) = ivars.next_button.borrow().load() {
                button.setContentTintColor(Some(&style_sheet.text_secondary));
            }
            if let Some(button) = ivars.reviewed_button.borrow().load() {
                button.setContentTintColor(Some(&style_sheet.change_color(ChangeKind::Inserted)));
            }
            if let Some(layer) = self.layer() {
                layer.setCornerRadius(Self::CORNER_RADIUS);
            }
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            let style_sheet = self.style_sheet();
            let shape = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                self.bounds(),
                Self::CORNER_RADIUS,
                Self::CORNER_RADIUS,
            );
            style_sheet.background.setFill();
            shape.fill();
            style_sheet
                .text
                .colorWithAlphaComponent(if style_sheet.increase_contrast { 0.10 } else { 0.055 })
                .setFill();
            shape.fill();

            style_sheet.rule.colorWithAlphaComponent(0.7).setStroke();
            shape.setLineWidth(PanelMetrics::HAIRLINE);
            shape.stroke();
        }
    }
);

impl ChangeSummaryBarView {
    const MINIMUM_WIDTH: CGFloat = 190.0;
    const MAXIMUM_WIDTH: CGFloat = 330.0;
    pub const TOAST_HEIGHT: CGFloat = 38.0;
    const CORNER_RADIUS: CGFloat = 19.0;

    /// `ChangeSummaryBarView()`: hosts build panels before they have a theme
    /// in hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<ChangeSummaryBarView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<ChangeSummaryBarView> {
        let stripe_color = style_sheet.change_color(ChangeKind::Inserted);
        let this = Self::alloc(mtm).set_ivars(ChangeSummaryBarViewIvars {
            delegate: RefCell::new(None),
            summary: RefCell::new(Summary::from_change_count(0)),
            current_position: Cell::new(None),
            previous_button: RefCell::new(ObjcWeak::default()),
            next_button: RefCell::new(ObjcWeak::default()),
            reviewed_button: RefCell::new(ObjcWeak::default()),
        });
        MessageBarView::stage_init(MessageBarInit { style_sheet, stripe_color });
        let this: Retained<ChangeSummaryBarView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.use_review_bar_layout();

        this.set_message("Updated on disk");
        let weak: ObjcWeak<ChangeSummaryBarView> = ObjcWeak::from(&*this);
        let previous_button = this.add_symbol_action("chevron.up", "Previous change", move || {
            let Some(this) = weak.load() else { return };
            this.advance_position(false);
            if let Some(delegate) = this.delegate() {
                delegate.change_summary_bar_did_request_jump(&this, false);
            }
        });
        let weak: ObjcWeak<ChangeSummaryBarView> = ObjcWeak::from(&*this);
        let next_button = this.add_symbol_action("chevron.down", "Next change", move || {
            let Some(this) = weak.load() else { return };
            this.advance_position(true);
            if let Some(delegate) = this.delegate() {
                delegate.change_summary_bar_did_request_jump(&this, true);
            }
        });
        for button in [&previous_button, &next_button] {
            button.widthAnchor().constraintEqualToConstant(28.0).setActive(true);
        }
        *this.ivars().previous_button.borrow_mut() = ObjcWeak::from(&*previous_button);
        *this.ivars().next_button.borrow_mut() = ObjcWeak::from(&*next_button);
        // The stack reads as two groups: the chevrons pair into one walk
        // control, and a wider gap sets the finishing action apart from it.
        this.set_action_spacing(0.0, &previous_button);
        this.set_action_spacing(10.0, &next_button);
        this.update_navigation_state();

        let weak: ObjcWeak<ChangeSummaryBarView> = ObjcWeak::from(&*this);
        let reviewed_button = this.add_symbol_action("checkmark", "Mark changes as reviewed", move || {
            if let Some(this) = weak.load()
                && let Some(delegate) = this.delegate()
            {
                delegate.change_summary_bar_did_request_mark_reviewed(&this);
            }
        });
        reviewed_button.setBordered(false);
        reviewed_button.setToolTip(Some(&ns_string("Clear unread change marks")));
        *this.ivars().reviewed_button.borrow_mut() = ObjcWeak::from(&*reviewed_button);
        let weak: ObjcWeak<ChangeSummaryBarView> = ObjcWeak::from(&*this);
        this.set_on_dismiss(Some(Rc::new(move || {
            if let Some(this) = weak.load()
                && let Some(delegate) = this.delegate()
            {
                delegate.change_summary_bar_did_request_dismiss(&this);
            }
        })));

        // This is a transient notice, not a review toolbar.
        Self::hide_buttons(&this);
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setMasksToBounds(true);
        }
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(Self::CORNER_RADIUS);
        }

        set_label(&*this, "Document updated on disk");
        // The base's own `applyStyle` ran before these buttons existed.
        this.apply_style();
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn ChangeSummaryBarDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn ChangeSummaryBarDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    fn change_count(&self) -> isize {
        self.ivars().summary.borrow().total()
    }

    fn hide_buttons(view: &NSView) {
        for subview in view.subviews().iter() {
            if let Some(button) = subview.downcast_ref::<NSButton>() {
                button.setHidden(true);
            }
            Self::hide_buttons(&subview);
        }
    }

    /// `configure(message:summary:)`: configures the bar from the tracker's
    /// marks. A `message` overrides the derived headline.
    pub fn configure(&self, message: Option<&str>, summary: Summary) {
        let description = summary.accessibility_description();
        let headline = summary.headline();
        *self.ivars().summary.borrow_mut() = summary;
        self.set_message(message.unwrap_or(&headline));
        self.ivars().current_position.set(None);
        self.update_position_status();
        self.update_navigation_state();
        self.invalidateIntrinsicContentSize();
        let label = match message {
            Some(message) => format!("{message}. {description}"),
            None => description,
        };
        set_label(self, &label);
        self.setNeedsDisplay(true);
    }

    /// `configure(message:changeCount:)`: the count-only entry point.
    pub fn configure_count(&self, message: &str, change_count: isize) {
        *self.ivars().summary.borrow_mut() = Summary::from_change_count(change_count);
        self.set_message(message);
        self.ivars().current_position.set(None);
        self.update_position_status();
        self.update_navigation_state();
        self.invalidateIntrinsicContentSize();
        set_label(self, &format!("{message}. {change_count} unread changes"));
        self.setNeedsDisplay(true);
    }

    /// Walking is only offered when there is something to walk (§11.4).
    fn update_navigation_state(&self) {
        let can_walk = self.change_count() > 0;
        let ivars = self.ivars();
        let previous = ivars.previous_button.borrow().load();
        let next = ivars.next_button.borrow().load();
        if let Some(button) = &previous {
            button.setEnabled(can_walk);
        }
        if let Some(button) = &next {
            button.setEnabled(can_walk);
        }
        let help = if can_walk { "" } else { " (no unread changes)" };
        if let Some(button) = &previous {
            button.setToolTip(Some(&ns_string(&format!("Previous change{help}"))));
        }
        if let Some(button) = &next {
            button.setToolTip(Some(&ns_string(&format!("Next change{help}"))));
        }
    }

    /// `positionStatusForTesting`.
    pub fn position_status_for_testing(&self) -> String {
        self.ivars()
            .current_position
            .get()
            .map(|position| format!("{position} of {}", self.change_count()))
            .unwrap_or_default()
    }

    fn advance_position(&self, forward: bool) {
        let count = self.change_count();
        if !(count > 0) {
            return;
        }
        let next = match (self.ivars().current_position.get(), forward) {
            (None, true) => 1,
            (None, false) => count,
            (Some(current), true) => {
                if current == count {
                    1
                } else {
                    current + 1
                }
            }
            (Some(current), false) => {
                if current == 1 {
                    count
                } else {
                    current - 1
                }
            }
        };
        self.ivars().current_position.set(Some(next));
        self.update_position_status();
    }

    fn update_position_status(&self) {
        self.set_status(&self.position_status_for_testing());
    }
}
