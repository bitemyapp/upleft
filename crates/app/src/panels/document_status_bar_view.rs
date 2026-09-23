//! Port of `Panels/DocumentStatusBarView.swift`: an **opt-in** footer strip
//! showing the live caret position.
//!
//! DESIGN.md's "Avoid" list names a permanent status bar outright, so this is
//! off by default and shown only when `Preferences.showStatusBar` asks for it
//! (View ▸ Show Status Bar). What survives that constraint is the one figure
//! nothing else in the app reports — line and column while editing — plus
//! the unsaved marker.
//!
//! The bar never takes the first responder and never scrolls — it is pinned
//! to the bottom of the document root view, just above the window's bottom
//! edge.
//!
//! Reproduced as Swift has it: the accent dot is never hidden, whatever
//! `hasFileURL` says, and the label's leading constraint is chosen once, at
//! init, when `hasFileURL` is still its default `true`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSFont, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLineBreakMode, NSResponder, NSTextField,
    NSTrackingArea, NSView, NSViewNoIntrinsicMetric,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSRect, NSSize};
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{activate, cg, label, ns_string, role, set_label, set_role, set_value, weight_regular};
use super::panel_chrome::PanelMetrics;

/// `DocumentStatusBarView.Metrics`.
struct Metrics;

impl Metrics {
    const HEIGHT: CGFloat = 22.0;
    const PADDING: CGFloat = 12.0;
    const INSET_X: CGFloat = 10.0;
    const TOTAL_HEIGHT: CGFloat = Self::HEIGHT + Self::PADDING;
    const DOT_SIZE: CGFloat = 5.0;
    const DOT_GAP: CGFloat = 6.0;
}

pub struct DocumentStatusBarViewIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    /// Set from the text view's selection change handler.
    cursor_position: Cell<Option<(isize, isize)>>,
    /// Whether the document has ever been saved to disk. An unsaved new
    /// document shows a different cue so the reader knows saving is
    /// required.
    has_file_url: Cell<bool>,
    is_visible: Cell<bool>,
    cursor_label: Retained<NSTextField>,
    unsaved_dot: Retained<NSView>,
    divider: Retained<NSView>,
    #[allow(dead_code)]
    tracking_area_ref: RefCell<Option<Retained<NSTrackingArea>>>,
}

define_class!(
    /// `DocumentStatusBarView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentStatusBarView"]
    #[ivars = DocumentStatusBarViewIvars]
    pub struct DocumentStatusBarView;

    unsafe impl NSObjectProtocol for DocumentStatusBarView {}

    impl DocumentStatusBarView {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(
                unsafe { NSViewNoIntrinsicMetric },
                if self.ivars().is_visible.get() { Metrics::TOTAL_HEIGHT } else { 0.0 },
            )
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }
);

impl DocumentStatusBarView {
    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<DocumentStatusBarView> {
        // Stored-property initial values, in declaration order.
        let cursor_label = label("", mtm);
        let unsaved_dot = NSView::new(mtm);
        let divider = NSView::new(mtm);
        let this = Self::alloc(mtm).set_ivars(DocumentStatusBarViewIvars {
            style_sheet: RefCell::new(style_sheet),
            cursor_position: Cell::new(None),
            has_file_url: Cell::new(true),
            is_visible: Cell::new(true),
            cursor_label: cursor_label.clone(),
            unsaved_dot: unsaved_dot.clone(),
            divider: divider.clone(),
            tracking_area_ref: RefCell::new(None),
        });
        let this: Retained<DocumentStatusBarView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        this.setTranslatesAutoresizingMaskIntoConstraints(false);

        cursor_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, weight_regular())));
        cursor_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        cursor_label
            .setContentHuggingPriority_forOrientation(NSLayoutPriorityDefaultLow, NSLayoutConstraintOrientation::Horizontal);
        this.addSubview(&cursor_label);

        unsaved_dot.setWantsLayer(true);
        if let Some(layer) = unsaved_dot.layer() {
            layer.setCornerRadius(Metrics::DOT_SIZE / 2.0);
        }
        this.addSubview(&unsaved_dot);

        divider.setWantsLayer(true);
        this.addSubview(&divider);

        // One-pass layout: the divider runs full width, the cursor sits on
        // the left, metrics on the right. `translatesAutoresizingMaskIntoConstraints`
        // is off so we use Auto Layout.
        cursor_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        unsaved_dot.setTranslatesAutoresizingMaskIntoConstraints(false);
        divider.setTranslatesAutoresizingMaskIntoConstraints(false);

        let has_file_url = this.ivars().has_file_url.get();
        let leading_anchor = if has_file_url { unsaved_dot.trailingAnchor() } else { this.leadingAnchor() };
        activate(&[
            divider.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            divider.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            divider.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            divider.heightAnchor().constraintEqualToConstant(PanelMetrics::HAIRLINE),
            unsaved_dot.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), Metrics::INSET_X),
            unsaved_dot.centerYAnchor().constraintEqualToAnchor_constant(&this.centerYAnchor(), Metrics::PADDING / 2.0),
            unsaved_dot.widthAnchor().constraintEqualToConstant(Metrics::DOT_SIZE),
            unsaved_dot.heightAnchor().constraintEqualToConstant(Metrics::DOT_SIZE),
            cursor_label.leadingAnchor().constraintEqualToAnchor_constant(
                &leading_anchor,
                if has_file_url { Metrics::DOT_GAP } else { Metrics::INSET_X },
            ),
            cursor_label.centerYAnchor().constraintEqualToAnchor_constant(&this.centerYAnchor(), Metrics::PADDING / 2.0),
            cursor_label
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&this.trailingAnchor(), -Metrics::INSET_X),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Document status");
        this.apply_style();
        this
    }

    // MARK: - Properties

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn cursor_position(&self) -> Option<(isize, isize)> {
        self.ivars().cursor_position.get()
    }

    /// `cursorPosition = (line, column)`.
    pub fn set_cursor_position(&self, cursor_position: Option<(isize, isize)>) {
        let old_value = self.ivars().cursor_position.replace(cursor_position);
        if cursor_position.map(|position| position.0) == old_value.map(|position| position.0)
            && cursor_position.map(|position| position.1) == old_value.map(|position| position.1)
        {
            return;
        }
        self.update_cursor_label();
    }

    pub fn has_file_url(&self) -> bool {
        self.ivars().has_file_url.get()
    }

    pub fn set_has_file_url(&self, has_file_url: bool) {
        let old_value = self.ivars().has_file_url.replace(has_file_url);
        if has_file_url == old_value {
            return;
        }
        self.update_cursor_label();
    }

    /// `isVisible` (the Swift stored property, not `NSWindow`'s).
    pub fn is_visible(&self) -> bool {
        self.ivars().is_visible.get()
    }

    pub fn set_is_visible(&self, is_visible: bool) {
        let old_value = self.ivars().is_visible.replace(is_visible);
        if is_visible == old_value {
            return;
        }
        self.setHidden(!is_visible);
        self.invalidateIntrinsicContentSize();
    }

    // MARK: - Updates

    fn update_cursor_label(&self) {
        let ivars = self.ivars();
        let Some((line, column)) = ivars.cursor_position.get() else {
            ivars.cursor_label.setStringValue(&ns_string(""));
            set_value(self, "");
            return;
        };
        let prefix = if ivars.has_file_url.get() { "" } else { "Unsaved · " };
        ivars.cursor_label.setStringValue(&ns_string(&format!("{prefix}Ln {line}, Col {column}")));
        let text = ivars.cursor_label.stringValue();
        let _: () = unsafe { msg_send![&*ivars.cursor_label, setAccessibilityLabel: &*text] };
        self.apply_style();
    }

    // MARK: - Style

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        let contrast = style_sheet.increase_contrast;
        if let Some(layer) = ivars.divider.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.rule.colorWithAlphaComponent(if contrast {
                0.6
            } else {
                0.35
            }))));
        }
        // `textSecondary`, not `textFaint`. Faint is the marker tier — it
        // measures 3.35:1 against the warm dark page, well under WCAG AA's
        // 4.5:1, and at this bar's 11pt the readout was effectively
        // invisible. Secondary clears AA at 6.25:1 and is still unmistakably
        // chrome (§11.4).
        ivars.cursor_label.setTextColor(Some(&style_sheet.text_secondary));
        if let Some(layer) = ivars.unsaved_dot.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.accent)));
        }
        self.setNeedsDisplay(true);
    }

    // MARK: - Test hooks

    pub fn cursor_label_for_testing(&self) -> Retained<NSTextField> {
        self.ivars().cursor_label.clone()
    }
}
