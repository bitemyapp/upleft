//! Port of `Panels/PanelChrome.swift`: chrome shared by every summonable
//! surface (§11.4).
//!
//! "No permanent sidebars, panels, or status bar.  Everything summonable,
//! nothing resident."  Nothing in this directory may assume it is on screen:
//! a panel reports the width a host should animate it to, holds no document
//! state of its own, and draws from the `StyleSheet` it is handed rather than
//! caching colours — which is also what makes a theme change a property
//! assignment instead of a rebuild.
//!
//! Objective-C class names equal the Swift ones: `PanelStatusBadge`,
//! `PanelBackdrop`, `PanelTableView`, `PanelGroupRowView`, `ButtonAction`,
//! `PanelSymbolButton`, `MessageBarView`, `PanelSegmentedControl`,
//! `PanelProgressBar`, `PanelCheckbox`, `PanelSelectionRowView`,
//! `PanelEmptyStateView`.
//!
//! Swift's `didSet` properties are `set_…` methods that run the same body;
//! overridable Swift methods (`MessageBarView.applyStyle`) are Objective-C
//! methods so a subclass's override is what the base calls.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSApplication, NSAutoresizingMaskOptions, NSBezelStyle,
    NSBezierPath, NSBorderType, NSButton, NSButtonType, NSCellImagePosition, NSColor, NSControl, NSControlSize, NSEvent,
    NSFocusRingType, NSFont, NSFontWeight, NSHapticFeedbackManager, NSHapticFeedbackPattern,
    NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer, NSImage, NSImageScaling, NSImageView,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutAttribute, NSLayoutRelation, NSLineBreakMode, NSMenu,
    NSResponder, NSScrollView, NSStackView, NSTableColumn, NSTableColumnResizingOptions, NSTableRowView, NSTableView,
    NSTableViewColumnAutoresizingStyle, NSTableViewGridLineStyle, NSTableViewSelectionHighlightStyle,
    NSTableViewStyle, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView, NSViewNoIntrinsicMetric,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    NSWorkspace, NSStackViewGravity, NSAttributedStringNSStringDrawing, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired,
};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize, CGVector};
use objc2_core_graphics::{CGMutablePath, CGPath};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRange, NSRect, NSSize, NSString, NSValue};
use objc2_quartz_core::{
    NSValueCATransform3DAdditions, CAAnimation, CAAnimationGroup, CABasicAnimation, CAKeyframeAnimation, CALayer, CAMediaTiming, CAShapeLayer,
    CATextLayer, CATextLayerAlignmentMode, CATransaction, CATransform3D, CATransform3DIdentity,
    kCALineCapRound, kCALineJoinRound, kCAAlignmentCenter,
};
use upleft_render::engine::render_metrics;
use upleft_render::motion::{self, Curve, SpringDriver, SpringScalar};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;

use super::appkit_support::{Presentation, superview, needs_display, 
    IDENTITY, RectExt, activate, is_same_view, cg, configured_symbol, label, main_async, ns_string, null_actions, rect, rect_fill,
    role, set_label, set_role, set_value, smax, smin, symbol_configuration, weight_medium, weight_regular,
    weight_semibold, without_actions, wrapping_label,
};
use crate::app::toolbar_controls::ToolbarChromePolicy;
use crate::support::commands::{Command, KeyBinding};
use crate::support::preferences::Preferences;

// MARK: - PanelMetrics

/// `PanelMetrics`.
pub struct PanelMetrics;

impl PanelMetrics {
    // MARK: Widths
    //
    // Three tiers, not a number per panel.  A panel picks the shape its rows
    // need — a name, a name plus a detail line, or a working surface — and
    // the set of panel widths in the app stays three wide instead of eight.

    /// One line per row: outline, siblings, workspace, reader profiles.
    pub const LIST_WIDTH: CGFloat = 300.0;
    /// A title plus a detail line, and usually a row action: tasks, lens,
    /// health, render targets, assets, reviews, search results, front matter.
    pub const DETAIL_WIDTH: CGFloat = 336.0;
    /// A working surface that is not a trailing panel: table editor, palette.
    pub const WIDE_WIDTH: CGFloat = 520.0;

    // MARK: Row heights

    /// A single line of text.
    pub const LIST_ROW_HEIGHT: CGFloat = 30.0;
    /// A title over a detail line.
    pub const DETAIL_ROW_HEIGHT: CGFloat = 46.0;
    /// A title, a detail line, and a third line or an action row.
    pub const WIDE_ROW_HEIGHT: CGFloat = 60.0;
    /// Group headers sit under their own rule: they carry one small caption.
    pub const GROUP_ROW_HEIGHT: CGFloat = 22.0;

    /// Thin enough to replace a scrollbar rather than become a sidebar (§8.6).
    pub const GUTTER_WIDTH: CGFloat = 14.0;
    pub const BAR_HEIGHT: CGFloat = 32.0;
    pub const REVIEW_BAR_HEIGHT: CGFloat = 30.0;
    pub const INSET: CGFloat = 10.0;
    pub const HEADER_TOP_PADDING: CGFloat = 8.0;
    /// One radius family for detached glass and the surfaces nested inside it.
    pub const CORNER_RADIUS: CGFloat = 6.0;
    /// Detached glass needs enough curvature to read as a soft body at panel
    /// scale.
    pub const SURFACE_RADIUS: CGFloat = 20.0;
    /// Detached inspectors float inside the document rather than sharing the
    /// tighter radius used by compact toolbar plates.
    pub const FLOATING_SURFACE_RADIUS: CGFloat = 26.0;
    pub const FLOATING_MARGIN: CGFloat = 22.0;
    /// Space reserved by the transparent child window for the body's shadow.
    pub const FLOATING_SHADOW_MARGIN: CGFloat = 40.0;
    pub const NESTED_SURFACE_RADIUS: CGFloat = 10.0;
    pub const CHROME_PILL_RADIUS: CGFloat = 16.0;
    pub const BAND_CORNER_RADIUS: CGFloat = 12.0;
    pub const TOOLBAR_CONTROL_SIDE: CGFloat = 34.0;
    pub const HAIRLINE: CGFloat = 1.0;

    /// The shape a row's own surface takes — selection, hover, a completion
    /// wash (§11.4).
    pub fn row_surface(bounds: NSRect) -> NSRect {
        bounds.inset_by(3.0, 1.0)
    }

    pub const ROW_SURFACE_RADIUS: CGFloat = 10.0;

    /// A cubic superellipse approximation for masks that must match a
    /// `.continuous` layer corner.
    pub fn continuous_rounded_path(rect: CGRect, radius: CGFloat) -> CFRetained<CGPath> {
        let r = smin(smax(0.0, radius), smin(rect.width(), rect.height()) / 2.0);
        if !(r > 0.0) {
            // SAFETY: a null transform is allowed.
            return unsafe { CGPath::with_rect(rect, std::ptr::null()) };
        }
        let control = r * 0.447_715;
        let path = CGMutablePath::new();
        let p = Some(&*path);
        // SAFETY: `IDENTITY` outlives each call.
        unsafe {
            CGMutablePath::move_to_point(p, &IDENTITY, rect.min_x() + r, rect.min_y());
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.max_x() - r, rect.min_y());
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.max_x() - control,
                rect.min_y(),
                rect.max_x(),
                rect.min_y() + control,
                rect.max_x(),
                rect.min_y() + r,
            );
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.max_x(), rect.max_y() - r);
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.max_x(),
                rect.max_y() - control,
                rect.max_x() - control,
                rect.max_y(),
                rect.max_x() - r,
                rect.max_y(),
            );
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.min_x() + r, rect.max_y());
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.min_x() + control,
                rect.max_y(),
                rect.min_x(),
                rect.max_y() - control,
                rect.min_x(),
                rect.max_y() - r,
            );
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.min_x(), rect.min_y() + r);
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.min_x(),
                rect.min_y() + control,
                rect.min_x() + control,
                rect.min_y(),
                rect.min_x() + r,
                rect.min_y(),
            );
        }
        CGMutablePath::close_subpath(p);
        super::appkit_support::immutable(path)
    }

    /// A capsule's radius is half its height, including for tiny controls.
    pub fn capsule_radius(height: CGFloat) -> CGFloat {
        smax(0.0, height / 2.0)
    }

    /// Small controls use the same inset capsule as the ring's hover plate
    /// and the segmented switcher.
    pub fn control_radius(height: CGFloat) -> CGFloat {
        smin(Self::NESTED_SURFACE_RADIUS, Self::capsule_radius(height))
    }
}

// MARK: - PanelStatusBadge

define_class!(
    /// A count in transient chrome is context, not another action.  Give it
    /// a quiet, fixed shape so it cannot be confused with the neighbouring
    /// buttons.
    // SAFETY: no ivars; AppKit's own initialisers are safe to inherit.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelStatusBadge"]
    pub struct PanelStatusBadge;

    unsafe impl NSObjectProtocol for PanelStatusBadge {}

    impl PanelStatusBadge {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            let size: NSSize = unsafe { msg_send![super(self), intrinsicContentSize] };
            NSSize::new(size.width + 12.0, 20.0)
        }

        /// A pill, not a rounded rect.
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            if let Some(layer) = self.layer() {
                layer.setCornerRadius(self.bounds().height() / 2.0);
            }
        }
    }
);

impl PanelStatusBadge {
    /// `PanelStatusBadge(labelWithString:)`: NSTextField's class factory,
    /// sent to the subclass.
    pub fn label_with_string(text: &str, mtm: MainThreadMarker) -> Retained<PanelStatusBadge> {
        let _ = mtm;
        unsafe { msg_send![PanelStatusBadge::class(), labelWithString: &*ns_string(text)] }
    }
}

// MARK: - PanelSurface

/// A transient side surface.  `preferredWidth` exists so a host can animate
/// the panel in from zero without knowing what is inside it (§11.4).
///
/// A panel class that conforms also implements the Objective-C method
/// `preferredWidth` (returning `CGFloat`), which is how
/// `FloatingPanelSurface` performs Swift's `content as? PanelSurface`.
pub trait PanelSurface {
    fn preferred_width(&self) -> CGFloat;
}

/// `(view as? PanelSurface)?.preferredWidth`.
pub fn panel_surface_preferred_width(view: &NSView) -> Option<CGFloat> {
    super::appkit_support::cgfloat_if_responds(view, sel!(preferredWidth))
}

// MARK: - PanelFont

/// Panels are chrome, not the document, so their labels use the system face
/// at system sizes.  Colour always comes from the `StyleSheet`.
pub struct PanelFont;

impl PanelFont {
    fn adjustment() -> CGFloat {
        Preferences::shared().text_size_adjustment()
    }

    fn size(base: CGFloat) -> CGFloat {
        smax(11.0, smin(22.0, base + Self::adjustment()))
    }

    pub fn system(base: CGFloat, weight: NSFontWeight) -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(Self::size(base), weight)
    }

    /// `PanelFont.system(_:)` with the default `.regular` weight.
    pub fn system_regular(base: CGFloat) -> Retained<NSFont> {
        Self::system(base, weight_regular())
    }

    pub fn monospaced(base: CGFloat, weight: NSFontWeight) -> Retained<NSFont> {
        NSFont::monospacedSystemFontOfSize_weight(Self::size(base), weight)
    }

    /// `PanelFont.monospaced(_:)` with the default `.regular` weight.
    pub fn monospaced_regular(base: CGFloat) -> Retained<NSFont> {
        Self::monospaced(base, weight_regular())
    }

    pub fn row() -> Retained<NSFont> {
        NSFont::systemFontOfSize(Self::size(12.5))
    }

    pub fn task_row() -> Retained<NSFont> {
        NSFont::systemFontOfSize(Self::size(13.5))
    }

    pub fn row_emphasised() -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(Self::size(12.5), weight_semibold())
    }

    pub fn secondary() -> Retained<NSFont> {
        NSFont::systemFontOfSize(Self::size(11.5))
    }

    pub fn header() -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(Self::size(12.0), weight_semibold())
    }

    pub fn group() -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(Self::size(11.5), weight_semibold())
    }

    pub fn title() -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(Self::size(13.0), weight_semibold())
    }

    pub fn floating_title() -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(Self::size(13.0), weight_regular())
    }
}

// MARK: - PanelAnimation

/// Every animated transition in this directory goes through here, so there
/// is exactly one place Reduce Motion is honoured (§11.4).
pub struct PanelAnimation;

impl PanelAnimation {
    /// `PanelAnimation.run(reduceMotion:duration:_:completion:)`; the Swift
    /// default duration is `Motion.standard`.
    pub fn run(
        reduce_motion: bool,
        duration: f64,
        changes: impl Fn(&objc2_app_kit::NSAnimationContext) + 'static,
        completion: Option<Box<dyn Fn() + 'static>>,
    ) {
        motion::run(reduce_motion, duration, Curve::Decelerate, changes, completion);
    }
}

// MARK: - RelativeTime

/// `RelativeTime`: "2m", "3 minutes ago", and a medium/short stamp.
pub struct RelativeTime;

thread_local! {
    static ABBREVIATED_FORMATTER: Retained<objc2_foundation::NSRelativeDateTimeFormatter> = {
        let f = objc2_foundation::NSRelativeDateTimeFormatter::new();
        f.setUnitsStyle(objc2_foundation::NSRelativeDateTimeFormatterUnitsStyle::Abbreviated);
        f
    };
    static NAMED_FORMATTER: Retained<objc2_foundation::NSRelativeDateTimeFormatter> = {
        let f = objc2_foundation::NSRelativeDateTimeFormatter::new();
        f.setUnitsStyle(objc2_foundation::NSRelativeDateTimeFormatterUnitsStyle::Full);
        f.setDateTimeStyle(objc2_foundation::NSRelativeDateTimeFormatterStyle::Named);
        f
    };
    static STAMP_FORMATTER: Retained<objc2_foundation::NSDateFormatter> = {
        let f = objc2_foundation::NSDateFormatter::new();
        f.setDateStyle(objc2_foundation::NSDateFormatterStyle::MediumStyle);
        f.setTimeStyle(objc2_foundation::NSDateFormatterStyle::ShortStyle);
        f
    };
}

fn ns_date(date: upleft_foundation::date::Date) -> Retained<objc2_foundation::NSDate> {
    objc2_foundation::NSDate::dateWithTimeIntervalSinceReferenceDate(date.time_interval_since_reference_date)
}

impl RelativeTime {
    /// "2m", "3h" — for a list where the column is a few characters wide.
    pub fn short(date: upleft_foundation::date::Date, now: upleft_foundation::date::Date) -> String {
        if now.time_interval_since(date) < 60.0 {
            return "now".to_owned();
        }
        ABBREVIATED_FORMATTER.with(|f| f.localizedStringForDate_relativeToDate(&ns_date(date), &ns_date(now)).to_string())
    }

    /// "3 minutes ago", "yesterday".
    pub fn long(date: upleft_foundation::date::Date, now: upleft_foundation::date::Date) -> String {
        NAMED_FORMATTER.with(|f| f.localizedStringForDate_relativeToDate(&ns_date(date), &ns_date(now)).to_string())
    }

    pub fn stamp(date: upleft_foundation::date::Date) -> String {
        STAMP_FORMATTER.with(|f| f.stringFromDate(&ns_date(date)).to_string())
    }
}

// MARK: - PanelBackdrop

pub struct PanelBackdropIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    opaque_surface_color: RefCell<Option<Retained<NSColor>>>,
    uses_surface_fill: Cell<bool>,
    blends_within_window: Cell<bool>,
    veil_alpha: Cell<CGFloat>,
    effect: Retained<NSVisualEffectView>,
    veil_layer: Retained<CALayer>,
    is_inside_detached_glass: Cell<bool>,
}

define_class!(
    /// Panel background: native vibrancy, or a flat themed fill where Reduce
    /// Transparency or Increase Contrast is on (§11.4).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelBackdrop"]
    #[ivars = PanelBackdropIvars]
    pub struct PanelBackdrop;

    unsafe impl NSObjectProtocol for PanelBackdrop {}

    impl PanelBackdrop {
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.ivars().veil_layer.setFrame(self.bounds());
        }

        #[unsafe(method(viewDidMoveToSuperview))]
        fn __view_did_move_to_superview(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToSuperview] };
            self.resolve_detached_glass_visibility();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.resolve_detached_glass_visibility();
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            // The owning FloatingPanelSurface / ChromeGlass is the material.
            let ivars = self.ivars();
            if !(!ivars.is_inside_detached_glass.get() && (self.prefers_opaque() || ivars.uses_surface_fill.get())) {
                return;
            }
            let style_sheet = ivars.style_sheet.borrow().clone();
            let color = match ivars.opaque_surface_color.borrow().clone() {
                Some(color) => color,
                None => {
                    if ivars.uses_surface_fill.get() {
                        style_sheet.surface.clone()
                    } else {
                        style_sheet.background.clone()
                    }
                }
            };
            color.setFill();
            rect_fill(dirty_rect);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.setNeedsDisplay(true);
        }
    }
);

impl PanelBackdrop {
    /// `init(styleSheet:material:blendingMode:)`; Swift's defaults are
    /// `.sidebar` and `.behindWindow` (see [`PanelBackdrop::new_default`]).
    pub fn new(
        style_sheet: Rc<StyleSheet>,
        material: NSVisualEffectMaterial,
        blending_mode: NSVisualEffectBlendingMode,
        mtm: MainThreadMarker,
    ) -> Retained<PanelBackdrop> {
        let effect = NSVisualEffectView::new(mtm);
        let veil_layer = CALayer::new();
        let this = Self::alloc(mtm).set_ivars(PanelBackdropIvars {
            style_sheet: RefCell::new(style_sheet),
            opaque_surface_color: RefCell::new(None),
            uses_surface_fill: Cell::new(false),
            blends_within_window: Cell::new(false),
            veil_alpha: Cell::new(0.0),
            effect: effect.clone(),
            veil_layer: veil_layer.clone(),
            is_inside_detached_glass: Cell::new(false),
        });
        let this: Retained<PanelBackdrop> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        effect.setMaterial(material);
        effect.setBlendingMode(blending_mode);
        effect.setState(NSVisualEffectState::FollowsWindowActiveState);
        effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        effect.setFrame(this.bounds());
        this.addSubview(&effect);
        null_actions(&veil_layer, &["position", "bounds", "opacity", "backgroundColor"]);
        if let Some(layer) = this.layer() {
            layer.addSublayer(&veil_layer);
        }
        veil_layer.setOpacity(smin(smax(this.ivars().veil_alpha.get(), 0.0), 1.0) as f32);
        this.apply_style();
        this
    }

    /// `PanelBackdrop(styleSheet:)`: `.sidebar`, `.behindWindow`.
    pub fn new_default(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<PanelBackdrop> {
        Self::new(style_sheet, NSVisualEffectMaterial::Sidebar, NSVisualEffectBlendingMode::BehindWindow, mtm)
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn opaque_surface_color(&self) -> Option<Retained<NSColor>> {
        self.ivars().opaque_surface_color.borrow().clone()
    }

    pub fn set_opaque_surface_color(&self, color: Option<Retained<NSColor>>) {
        *self.ivars().opaque_surface_color.borrow_mut() = color;
        self.apply_style();
    }

    pub fn uses_surface_fill(&self) -> bool {
        self.ivars().uses_surface_fill.get()
    }

    pub fn set_uses_surface_fill(&self, value: bool) {
        self.ivars().uses_surface_fill.set(value);
        self.apply_style();
    }

    pub fn blends_within_window(&self) -> bool {
        self.ivars().blends_within_window.get()
    }

    pub fn set_blends_within_window(&self, value: bool) {
        self.ivars().blends_within_window.set(value);
        self.ivars().effect.setBlendingMode(if value {
            NSVisualEffectBlendingMode::WithinWindow
        } else {
            NSVisualEffectBlendingMode::BehindWindow
        });
        self.apply_style();
    }

    pub fn veil_alpha(&self) -> CGFloat {
        self.ivars().veil_alpha.get()
    }

    pub fn set_veil_alpha(&self, value: CGFloat) {
        self.ivars().veil_alpha.set(value);
        self.ivars().veil_layer.setOpacity(smin(smax(value, 0.0), 1.0) as f32);
        self.apply_style();
    }

    fn prefers_opaque(&self) -> bool {
        let style_sheet = self.ivars().style_sheet.borrow();
        style_sheet.reduce_transparency || style_sheet.increase_contrast
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let hidden = self.prefers_opaque() || ivars.uses_surface_fill.get() || ivars.is_inside_detached_glass.get();
        if hidden {
            ivars.effect.removeFromSuperview();
        } else if !is_same_view(superview(&ivars.effect), self) {
            ivars.effect.setFrame(self.bounds());
            self.addSubview_positioned_relativeTo(&ivars.effect, NSWindowOrderingMode::Below, None);
        }
        ivars.veil_layer.setHidden(hidden);
        let background = ivars.style_sheet.borrow().background.clone();
        ivars.veil_layer.setBackgroundColor(Some(&cg(&background)));
        self.setNeedsDisplay(true);
    }

    /// The floating surface owns the material and its content z-order.
    fn resolve_detached_glass_visibility(&self) {
        let mut ancestor = superview(&self);
        let mut resolved = false;
        while let Some(view) = ancestor {
            if super::appkit_support::is_kind_of(&view, c"FloatingPanelSurface")
                || super::appkit_support::is_kind_of(&view, c"ChromeGlass")
            {
                resolved = true;
                break;
            }
            ancestor = superview(&view);
        }
        self.ivars().is_inside_detached_glass.set(resolved);
        self.apply_style();
    }

    /// `PanelBackdrop.resolveDetachedGlass(in:)`.
    pub fn resolve_detached_glass(root: &NSView) {
        if let Some(backdrop) = super::appkit_support::downcast::<PanelBackdrop>(root) {
            backdrop.resolve_detached_glass_visibility();
        }
        for child in root.subviews().iter() {
            Self::resolve_detached_glass(&child);
        }
    }
}

// MARK: - PanelTableView

type KeyEventHandler = Rc<dyn Fn(&NSEvent) -> bool>;
type RowHandler = Rc<dyn Fn(isize) -> bool>;
type KeyHandler = Rc<dyn Fn(&str) -> bool>;
type MenuHandler = Rc<dyn Fn(isize) -> Option<Retained<NSMenu>>>;

#[derive(Default)]
pub struct PanelTableViewIvars {
    on_activate: RefCell<Option<Rc<dyn Fn()>>>,
    on_key_event: RefCell<Option<KeyEventHandler>>,
    on_row_mouse_down: RefCell<Option<RowHandler>>,
    on_key_down: RefCell<Option<KeyHandler>>,
    on_menu: RefCell<Option<MenuHandler>>,
}

define_class!(
    /// Table view that reports `⏎` on the selected row (§11.4).
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSTableView, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelTableView"]
    #[ivars = PanelTableViewIvars]
    pub struct PanelTableView;

    unsafe impl NSObjectProtocol for PanelTableView {}

    impl PanelTableView {
        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            let on_key_event = self.ivars().on_key_event.borrow().clone();
            if let Some(handler) = on_key_event
                && handler(event)
            {
                return;
            }
            let Some(key) = KeyBinding::key_for_event(event) else {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
                return;
            };
            let on_key_down = self.ivars().on_key_down.borrow().clone();
            if let Some(handler) = on_key_down
                && handler(&key)
            {
                return;
            }
            if key == "return" && self.selectedRow() >= 0 {
                let on_activate = self.ivars().on_activate.borrow().clone();
                if let Some(handler) = on_activate {
                    handler();
                }
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn __perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.perform_key_equivalent(event)
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            let row = self.rowAtPoint(self.convertPoint_fromView(event.locationInWindow(), None));
            let on_row_mouse_down = self.ivars().on_row_mouse_down.borrow().clone();
            if let Some(handler) = on_row_mouse_down
                && handler(row)
            {
                return;
            }
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
        }

        #[unsafe(method_id(menuForEvent:))]
        fn __menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            let row = self.rowAtPoint(self.convertPoint_fromView(event.locationInWindow(), None));
            let on_menu = self.ivars().on_menu.borrow().clone();
            on_menu.and_then(|handler| handler(row))
        }
    }
);

impl PanelTableView {
    /// `PanelTableView()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<PanelTableView> {
        let this = Self::alloc(mtm).set_ivars(PanelTableViewIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
        let on_key_event = self.ivars().on_key_event.borrow().clone();
        if let Some(handler) = on_key_event
            && handler(event)
        {
            return true;
        }
        unsafe { msg_send![super(self), performKeyEquivalent: event] }
    }

    pub fn set_on_activate(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_activate.borrow_mut() = handler;
    }

    pub fn set_on_key_event(&self, handler: Option<KeyEventHandler>) {
        *self.ivars().on_key_event.borrow_mut() = handler;
    }

    pub fn set_on_row_mouse_down(&self, handler: Option<RowHandler>) {
        *self.ivars().on_row_mouse_down.borrow_mut() = handler;
    }

    pub fn set_on_key_down(&self, handler: Option<KeyHandler>) {
        *self.ivars().on_key_down.borrow_mut() = handler;
    }

    pub fn set_on_menu(&self, handler: Option<MenuHandler>) {
        *self.ivars().on_menu.borrow_mut() = handler;
    }

    /// Runs `onActivate`, as the table does on Return.
    pub fn activate(&self) {
        let on_activate = self.ivars().on_activate.borrow().clone();
        if let Some(handler) = on_activate {
            handler();
        }
    }
}

// MARK: - PanelList

/// `PanelList`.
pub struct PanelList;

impl PanelList {
    pub fn make_scroll_view(document_view: &NSView, mtm: MainThreadMarker) -> Retained<NSScrollView> {
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setAutomaticallyAdjustsContentInsets(false);
        set_role(&*scroll, role::scroll_area());
        scroll.setDocumentView(Some(document_view));
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll
    }

    /// The one row-background every panel list uses (§11.4).
    pub fn selection_row(
        table_view: &NSTableView,
        owner: Option<&AnyObject>,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<PanelSelectionRowView> {
        let identifier = NSString::from_str("panelSelectionRow");
        let view = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, owner) }
            .and_then(|view| super::appkit_support::downcast::<PanelSelectionRowView>(&view))
            .unwrap_or_else(|| PanelSelectionRowView::new(mtm));
        view.setIdentifier(Some(&identifier));
        view.set_style_sheet(style_sheet);
        view
    }

    pub fn make_table_view(identifier: &str, mtm: MainThreadMarker) -> Retained<PanelTableView> {
        let table = PanelTableView::new(mtm);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns_string(identifier));
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&column);
        table.setHeaderView(None);
        table.setBackgroundColor(&NSColor::clearColor());
        table.setStyle(NSTableViewStyle::Plain);
        // Group rows are the panel's own `PanelGroupRowView` captions.
        table.setFloatsGroupRows(false);
        table.setGridStyleMask(NSTableViewGridLineStyle(0));
        table.setIntercellSpacing(NSSize::new(0.0, 0.0));
        table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::Regular);
        table.setAllowsEmptySelection(true);
        table.setAllowsMultipleSelection(false);
        table.setUsesAlternatingRowBackgroundColors(false);
        table.setColumnAutoresizingStyle(NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle);
        set_role(&*table, role::list());
        table.setFocusRingType(NSFocusRingType::Default);
        table
    }
}

// MARK: - PanelGroupRowView

pub struct PanelGroupRowViewIvars {
    label: Retained<NSTextField>,
    leading: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

define_class!(
    /// Group header used by the task panel, the sibling sidebar, the tidy
    /// sheet and the search results panel.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelGroupRowView"]
    #[ivars = PanelGroupRowViewIvars]
    pub struct PanelGroupRowView;

    unsafe impl NSObjectProtocol for PanelGroupRowView {}
);

impl PanelGroupRowView {
    /// `init(identifier:)`.
    pub fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<PanelGroupRowView> {
        let label = label("", mtm);
        let this = Self::alloc(mtm)
            .set_ivars(PanelGroupRowViewIvars { label: label.clone(), leading: RefCell::new(None) });
        let this: Retained<PanelGroupRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));

        label.setFont(Some(&PanelFont::group()));
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&label);

        let leading_constraint = label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET);
        activate(&[
            leading_constraint.clone(),
            label.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            label.bottomAnchor().constraintEqualToAnchor_constant(&this.bottomAnchor(), -3.0),
        ]);
        *this.ivars().leading.borrow_mut() = Some(leading_constraint);
        this
    }

    pub fn configure(&self, text: &str, color: &NSColor) {
        let label = &self.ivars().label;
        label.setStringValue(&ns_string(text));
        label.setTextColor(Some(color));
        set_role(self, role::row());
        set_label(self, text);
    }
}

// MARK: - ButtonAction

pub struct ButtonActionIvars {
    handler: Box<dyn Fn()>,
}

define_class!(
    /// Closure-backed button target.  Panels wire up a lot of one-line
    /// buttons and a selector per button would be noise; the panel keeps
    /// these alive.
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ButtonAction"]
    #[ivars = ButtonActionIvars]
    pub struct ButtonAction;

    unsafe impl NSObjectProtocol for ButtonAction {}

    impl ButtonAction {
        #[unsafe(method(fire:))]
        fn __fire(&self, _sender: Option<&AnyObject>) {
            (self.ivars().handler)();
        }
    }
);

impl ButtonAction {
    /// `ButtonAction(handler)`.
    pub fn new(handler: impl Fn() + 'static, mtm: MainThreadMarker) -> Retained<ButtonAction> {
        let this = Self::alloc(mtm).set_ivars(ButtonActionIvars { handler: Box::new(handler) });
        unsafe { msg_send![super(this), init] }
    }

    /// `ButtonAction { }`: a handler that does nothing, replaced later by
    /// re-targeting the button.
    pub fn noop(mtm: MainThreadMarker) -> Retained<ButtonAction> {
        Self::new(|| {}, mtm)
    }

    pub fn fire(&self) {
        (self.ivars().handler)();
    }

    /// `#selector(ButtonAction.fire(_:))`.
    pub fn selector() -> Sel {
        sel!(fire:)
    }
}

// MARK: - PanelButton

/// `PanelButton`.
pub struct PanelButton;

impl PanelButton {
    /// `PanelButton.symbol(_:label:action:pointSize:weight:firesOnMouseDown:)`;
    /// the Swift defaults are 13pt, `.medium`, `false`.
    pub fn symbol(
        name: &str,
        label: &str,
        action: &ButtonAction,
        point_size: CGFloat,
        weight: NSFontWeight,
        fires_on_mouse_down: bool,
        mtm: MainThreadMarker,
    ) -> Retained<NSButton> {
        let configuration = symbol_configuration(point_size, weight);
        let image = configured_symbol(name, Some(label), &configuration);
        let image = image.unwrap_or_else(|| NSImage::new());
        let button = PanelSymbolButton::with_image(&image, action, ButtonAction::selector(), mtm);
        button.ivars().fires_on_mouse_down.set(fires_on_mouse_down);
        button.setBordered(false);
        button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        button.setImagePosition(NSCellImagePosition::ImageOnly);
        button.setFocusRingType(NSFocusRingType::Default);
        set_label(&*button, label);
        set_role(&*button, role::button());
        button.setToolTip(Some(&ns_string(label)));
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        Retained::into_super(button)
    }

    /// `PanelButton.symbol(_:label:action:)` with the Swift defaults.
    pub fn symbol_default(name: &str, label: &str, action: &ButtonAction, mtm: MainThreadMarker) -> Retained<NSButton> {
        Self::symbol(name, label, action, 13.0, weight_medium(), false, mtm)
    }

    pub fn text(title: &str, action: &ButtonAction, is_default: bool, mtm: MainThreadMarker) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(&ns_string(title), Some(action), Some(ButtonAction::selector()), mtm)
        };
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Small);
        button.setFont(Some(&PanelFont::system_regular(12.0)));
        if is_default {
            button.setKeyEquivalent(&NSString::from_str("\r"));
        }
        button.setFocusRingType(NSFocusRingType::Default);
        set_label(&*button, title);
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        button
    }

    pub fn set_immediate_press_handler(button: &NSButton, handler: impl Fn() + 'static) {
        if let Some(symbol) = super::appkit_support::downcast::<PanelSymbolButton>(button) {
            *symbol.ivars().immediate_press_handler.borrow_mut() = Some(Rc::new(handler));
        }
    }

    /// On/off pill for the find bar's regex, case, whole-word and
    /// in-selection switches.
    pub fn toggle(title: &str, label: &str, action: &ButtonAction, mtm: MainThreadMarker) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(&ns_string(title), Some(action), Some(ButtonAction::selector()), mtm)
        };
        button.setButtonType(NSButtonType::PushOnPushOff);
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Small);
        button.setFont(Some(&PanelFont::system_regular(12.0)));
        button.setFocusRingType(NSFocusRingType::Default);
        set_label(&*button, label);
        set_role(&*button, role::check_box());
        button.setToolTip(Some(&ns_string(label)));
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        button
    }

    /// `(button as? PanelSymbolButton)?.styleSheet = styleSheet`.
    pub fn set_style_sheet(button: &NSButton, style_sheet: Rc<StyleSheet>) {
        if let Some(symbol) = super::appkit_support::downcast::<PanelSymbolButton>(button) {
            *symbol.ivars().style_sheet.borrow_mut() = style_sheet;
        }
    }
}

// MARK: - PanelSymbolButton

pub struct PanelSymbolButtonIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    fires_on_mouse_down: Cell<bool>,
    immediate_press_handler: RefCell<Option<Rc<dyn Fn()>>>,
    is_hovered: Cell<bool>,
    hover_tracking: RefCell<Option<Retained<NSTrackingArea>>>,
    wash: RefCell<SpringScalar>,
    wash_driver: RefCell<Option<SpringDriver>>,
}

impl Drop for PanelSymbolButtonIvars {
    fn drop(&mut self) {
        if let Some(driver) = self.wash_driver.get_mut().take() {
            driver.park();
        }
    }
}

define_class!(
    /// Keep the glyph visually small while giving keyboard and pointer users
    /// a comfortable target; answer the pointer with a tint-derived wash.
    // SAFETY: `initWithFrame:` sets the ivars, so AppKit's own class
    // factories (`+buttonWithImage:target:action:`) create valid instances.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelSymbolButton"]
    #[ivars = PanelSymbolButtonIvars]
    pub struct PanelSymbolButton;

    unsafe impl NSObjectProtocol for PanelSymbolButton {}

    impl PanelSymbolButton {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("PanelSymbolButton is created on the main thread");
            let this = this.set_ivars(PanelSymbolButtonIvars {
                style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
                fires_on_mouse_down: Cell::new(false),
                immediate_press_handler: RefCell::new(None),
                is_hovered: Cell::new(false),
                hover_tracking: RefCell::new(None),
                wash: RefCell::new(SpringScalar::with_value(0.0, motion::HOVER)),
                wash_driver: RefCell::new(None),
            });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn __accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            let size: NSSize = unsafe { msg_send![super(self), intrinsicContentSize] };
            NSSize::new(smax(28.0, size.width), smax(28.0, size.height))
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().hover_tracking,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveInActiveApp
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.ivars().is_hovered.set(true);
            self.retarget_wash();
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_hovered.set(false);
            self.retarget_wash();
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.animate_press(0.90, motion::PRESS_IN);
            if self.ivars().fires_on_mouse_down.get() {
                if !self.isEnabled() {
                    return;
                }
                let handler = self.ivars().immediate_press_handler.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                } else {
                    let action = self.action();
                    let target = self.target();
                    let _ = unsafe { self.sendAction_to(action, target.as_deref()) };
                }
                let weak: ObjcWeak<PanelSymbolButton> = ObjcWeak::from(self);
                main_async(move || {
                    if let Some(this) = weak.load() {
                        this.animate_press(1.0, motion::PRESS_OUT);
                    }
                });
                return;
            }
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            self.animate_press(1.0, motion::PRESS_OUT);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn __accessibility_perform_press(&self) -> bool {
            self.accessibility_perform_press()
        }

        #[unsafe(method(viewWillDraw))]
        fn __view_will_draw(&self) {
            let _: () = unsafe { msg_send![super(self), viewWillDraw] };
            let target = self.ivars().wash.borrow().target_value();
            if target != self.wash_target() {
                self.retarget_wash();
            }
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            let driver = self.ivars().wash_driver.borrow().clone();
            if let Some(driver) = driver {
                driver.view_did_move_to_window(self.window().as_deref());
            }
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            let alpha = self.ivars().wash.borrow().value();
            if alpha > 0.001 && self.isEnabled() {
                let tint = self.contentTintColor().unwrap_or_else(NSColor::labelColor);
                tint.colorWithAlphaComponent(alpha).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    self.bounds().inset_by(2.0, 2.0),
                    PanelMetrics::ROW_SURFACE_RADIUS,
                    PanelMetrics::ROW_SURFACE_RADIUS,
                )
                .fill();
            }
            let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
        }
    }
);

impl PanelSymbolButton {
    /// `PanelSymbolButton(image:target:action:)`: NSButton's class factory
    /// sent to the subclass.
    fn with_image(image: &NSImage, target: &AnyObject, action: Sel, mtm: MainThreadMarker) -> Retained<PanelSymbolButton> {
        let _ = mtm;
        unsafe { msg_send![PanelSymbolButton::class(), buttonWithImage: image, target: target, action: action] }
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    fn accessibility_perform_press(&self) -> bool {
        if !self.isEnabled() {
            return false;
        }
        let handler = self.ivars().immediate_press_handler.borrow().clone();
        if let Some(handler) = handler {
            handler();
            return true;
        }
        unsafe { msg_send![super(self), accessibilityPerformPress] }
    }

    /// A disabled button washes to nothing.
    fn wash_target(&self) -> CGFloat {
        if !(self.ivars().is_hovered.get() && self.isEnabled()) {
            return 0.0;
        }
        let boost = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast();
        if self.isHighlighted() {
            if boost { 0.18 } else { 0.12 }
        } else if boost {
            0.12
        } else {
            0.07
        }
    }

    fn animate_press(&self, scale: CGFloat, duration: f64) {
        self.setWantsLayer(true);
        let transform = CATransform3D::new_scale(scale, scale, 1.0);
        if self.ivars().style_sheet.borrow().reduce_motion {
            if let Some(layer) = self.layer() {
                layer.setTransform(transform);
            }
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        let layer = self.layer();
        let from = layer
            .as_ref()
            .and_then(|layer| layer.__presentation())
            .map(|presentation| presentation.transform())
            .or_else(|| layer.as_ref().map(|layer| layer.transform()));
        unsafe {
            animation.setFromValue(from.map(transform_value).as_deref().map(|value| &**value as &AnyObject));
            animation.setToValue(Some(&transform_value(transform)));
        }
        animation.setDuration(duration);
        animation.setTimingFunction(Some(&motion::timing(Curve::Decelerate)));
        if let Some(layer) = layer {
            layer.setTransform(transform);
            layer.addAnimation_forKey(&animation, Some(&NSString::from_str("panel-button-press")));
        }
    }

    fn retarget_wash(&self) {
        let target = self.wash_target();
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        let window = self.window();
        let can_animate =
            !reduce_motion && window.as_ref().is_some_and(|window| window.isVisible() && window.screen().is_some());
        if !can_animate {
            let driver = self.ivars().wash_driver.borrow().clone();
            if let Some(driver) = driver {
                driver.park();
            }
            self.ivars().wash.borrow_mut().snap(target);
            needs_display(&self);
            return;
        }
        self.ivars().wash.borrow_mut().target(target);
        let driver = self.ivars().wash_driver.borrow().clone();
        let driver = driver.unwrap_or_else(|| self.make_wash_driver());
        if !driver.arm() {
            self.ivars().wash.borrow_mut().snap(self.wash_target());
            needs_display(&self);
        }
    }

    fn make_wash_driver(&self) -> SpringDriver {
        let weak_advance: ObjcWeak<PanelSymbolButton> = ObjcWeak::from(self);
        let weak_apply = weak_advance.clone();
        let driver = SpringDriver::new(
            self,
            move |dt| match weak_advance.load() {
                Some(this) => this.ivars().wash.borrow_mut().advance(dt),
                None => false,
            },
            move || {
                if let Some(this) = weak_apply.load() {
                    needs_display(&this);
                }
            },
        );
        *self.ivars().wash_driver.borrow_mut() = Some(driver.clone());
        driver
    }
}

fn transform_value(transform: CATransform3D) -> Retained<NSValue> {
    // SAFETY: a plain value conversion.
    unsafe { NSValue::valueWithCATransform3D(transform) }
}

/// `layer.presentation()?.transform ?? layer.transform` as an animation's
/// `fromValue`.
pub fn presentation_transform(layer: &CALayer) -> CATransform3D {
    layer.__presentation().map(|presentation| presentation.transform()).unwrap_or_else(|| layer.transform())
}

/// `animation.fromValue = …; animation.toValue = …` for a `CATransform3D`.
pub fn set_transform_values(animation: &CABasicAnimation, from: CATransform3D, to: CATransform3D) {
    unsafe {
        animation.setFromValue(Some(&transform_value(from)));
        animation.setToValue(Some(&transform_value(to)));
    }
}

/// `animation.fromValue = …; animation.toValue = …` for a `Float` /
/// `CGFloat` key path.
pub fn set_number_values(animation: &CABasicAnimation, from: Option<f64>, to: f64) {
    unsafe {
        animation.setFromValue(from.map(NSNumber::new_f64).as_deref().map(|value| &**value as &AnyObject));
        animation.setToValue(Some(&NSNumber::new_f64(to)));
    }
}

/// `animation.fromValue = …; animation.toValue = …` for a `CGColor`.
pub fn set_color_values(
    animation: &CABasicAnimation,
    from: Option<&objc2_core_graphics::CGColor>,
    to: &objc2_core_graphics::CGColor,
) {
    unsafe {
        let from: Option<&AnyObject> = from.map(|color| &*(color as *const objc2_core_graphics::CGColor as *const AnyObject));
        animation.setFromValue(from);
        animation.setToValue(Some(&*(to as *const objc2_core_graphics::CGColor as *const AnyObject)));
    }
}

/// `NSView.refreshTrackingArea(_:options:)`.
pub fn refresh_tracking_area(
    view: &NSView,
    area: &RefCell<Option<Retained<NSTrackingArea>>>,
    options: NSTrackingAreaOptions,
) {
    upleft_render::view::tracking_area::refresh_tracking_area(view, area, options);
}

// MARK: - MessageBarView

pub struct MessageBarViewIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    message: RefCell<String>,
    stripe_color: RefCell<Retained<NSColor>>,
    on_dismiss: RefCell<Option<Rc<dyn Fn()>>>,
    label: Retained<NSTextField>,
    status_label: Retained<PanelStatusBadge>,
    action_stack: Retained<NSStackView>,
    actions: RefCell<Vec<Retained<ButtonAction>>>,
    close_button: RefCell<Option<Retained<NSButton>>>,
}

/// The arguments of `MessageBarView.init(styleSheet:stripeColor:)`, handed
/// to `initWithFrame:` so a subclass (`ConflictBarView`,
/// `ChangeSummaryBarView`) initialises the base exactly as Swift's
/// `super.init(styleSheet:stripeColor:)` does.
pub struct MessageBarInit {
    pub style_sheet: Rc<StyleSheet>,
    pub stripe_color: Retained<NSColor>,
}

thread_local! {
    static PENDING_MESSAGE_BAR: RefCell<Option<MessageBarInit>> = const { RefCell::new(None) };
}

define_class!(
    /// The shape both §8.1 bars take: a message, a few actions, and a
    /// dismiss.  **Never a sheet, never a dialog.**
    // SAFETY: `initWithFrame:` sets the ivars from the pending init
    // arguments (see `MessageBarInit`).
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "MessageBarView"]
    #[ivars = MessageBarViewIvars]
    pub struct MessageBarView;

    unsafe impl NSObjectProtocol for MessageBarView {}

    impl MessageBarView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("MessageBarView is created on the main thread");
            let init = PENDING_MESSAGE_BAR.with(|pending| pending.borrow_mut().take()).unwrap_or_else(|| {
                let style_sheet = Rc::new(StyleSheet::current(mtm));
                let stripe_color = style_sheet.accent.clone();
                MessageBarInit { style_sheet, stripe_color }
            });
            let this = this.set_ivars(MessageBarViewIvars {
                style_sheet: RefCell::new(init.style_sheet),
                message: RefCell::new(String::new()),
                stripe_color: RefCell::new(init.stripe_color),
                on_dismiss: RefCell::new(None),
                label: label("", mtm),
                status_label: PanelStatusBadge::label_with_string("", mtm),
                action_stack: NSStackView::new(mtm),
                actions: RefCell::new(Vec::new()),
                close_button: RefCell::new(None),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.finish_init(mtm);
            this
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(unsafe { NSViewNoIntrinsicMetric }, PanelMetrics::BAR_HEIGHT)
        }

        /// Bars sit above the document, so they must not become first
        /// responder on a click that was meant for the text underneath.
        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            false
        }

        /// Overridable (`ConflictBarView`, `ChangeSummaryBarView`).
        #[unsafe(method(applyStyle))]
        fn __apply_style(&self) {
            self.base_apply_style();
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            let style_sheet = self.style_sheet();
            let bounds = self.bounds();
            style_sheet.background.setFill();
            rect_fill(bounds);
            style_sheet
                .text
                .colorWithAlphaComponent(if style_sheet.increase_contrast { 0.08 } else { 0.035 })
                .setFill();
            rect_fill(bounds);

            self.ivars().stripe_color.borrow().setFill();
            rect_fill(rect(0.0, 0.0, MessageBarView::STRIPE_WIDTH, bounds.height()));

            // A hairline rather than a border (§11.3).
            style_sheet.rule.setFill();
            rect_fill(rect(0.0, 0.0, bounds.width(), PanelMetrics::HAIRLINE));
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }
);

impl MessageBarView {
    const STRIPE_WIDTH: CGFloat = 2.0;

    /// `init(styleSheet:stripeColor:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, stripe_color: Retained<NSColor>, mtm: MainThreadMarker) -> Retained<MessageBarView> {
        Self::stage_init(MessageBarInit { style_sheet, stripe_color });
        unsafe { msg_send![MessageBarView::alloc(mtm), initWithFrame: NSRect::ZERO] }
    }

    /// Stages the base initialiser's arguments for a subclass about to send
    /// `initWithFrame:` to `super`.
    pub fn stage_init(init: MessageBarInit) {
        PENDING_MESSAGE_BAR.with(|pending| *pending.borrow_mut() = Some(init));
    }

    fn finish_init(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let label = &ivars.label;
        label.setFont(Some(&PanelFont::row()));
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        self.addSubview(label);

        let status_label = &ivars.status_label;
        status_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, weight_medium())));
        status_label.setTextColor(Some(&self.style_sheet().text_faint));
        status_label.setAlignment(NSTextAlignment::Center);
        status_label.setWantsLayer(true);
        status_label.setHidden(true);
        status_label.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let action_stack = &ivars.action_stack;
        action_stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        action_stack.setSpacing(4.0);
        action_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        action_stack.addArrangedSubview(status_label);
        self.addSubview(action_stack);

        let weak: ObjcWeak<MessageBarView> = ObjcWeak::from(self);
        let dismiss = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    let handler = this.ivars().on_dismiss.borrow().clone();
                    if let Some(handler) = handler {
                        handler();
                    }
                }
            },
            mtm,
        );
        ivars.actions.borrow_mut().push(dismiss.clone());
        let close = PanelButton::symbol_default("xmark", "Dismiss", &dismiss, mtm);
        *ivars.close_button.borrow_mut() = Some(close.clone());
        self.addSubview(&close);

        activate(&[
            label.leadingAnchor().constraintEqualToAnchor_constant(
                &self.leadingAnchor(),
                PanelMetrics::INSET + Self::STRIPE_WIDTH,
            ),
            label.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            action_stack.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&label.trailingAnchor(), 12.0),
            action_stack.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            close.leadingAnchor().constraintEqualToAnchor_constant(&action_stack.trailingAnchor(), 8.0),
            close.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            close.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            close.widthAnchor().constraintEqualToConstant(28.0),
        ]);

        set_role(self, role::group());
        self.apply_style();
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn message(&self) -> String {
        self.ivars().message.borrow().clone()
    }

    pub fn set_message(&self, message: &str) {
        *self.ivars().message.borrow_mut() = message.to_owned();
        self.ivars().label.setStringValue(&ns_string(message));
        set_label(self, message);
        self.invalidateIntrinsicContentSize();
    }

    /// Width that actually fits this bar's message and its actions.
    pub fn fitted_width(&self) -> CGFloat {
        let ivars = self.ivars();
        let label_width = ivars.label.attributedStringValue().size().width.ceil();
        let actions_width = ivars.action_stack.fittingSize().width.ceil();
        PanelMetrics::INSET + Self::STRIPE_WIDTH + label_width + 12.0 + actions_width + 8.0 + 28.0 + PanelMetrics::INSET
    }

    pub fn stripe_color(&self) -> Retained<NSColor> {
        self.ivars().stripe_color.borrow().clone()
    }

    pub fn set_stripe_color(&self, color: Retained<NSColor>) {
        *self.ivars().stripe_color.borrow_mut() = color;
        self.setNeedsDisplay(true);
    }

    pub fn set_on_dismiss(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_dismiss.borrow_mut() = handler;
    }

    pub fn add_action(&self, title: &str, handler: impl Fn() + 'static) -> Retained<NSButton> {
        let mtm = self.mtm();
        let action = ButtonAction::new(handler, mtm);
        self.ivars().actions.borrow_mut().push(action.clone());
        let button = PanelButton::text(title, &action, false, mtm);
        self.ivars().action_stack.addArrangedSubview(&button);
        button
    }

    pub fn add_symbol_action(&self, symbol: &str, label: &str, handler: impl Fn() + 'static) -> Retained<NSButton> {
        let mtm = self.mtm();
        let action = ButtonAction::new(handler, mtm);
        self.ivars().actions.borrow_mut().push(action.clone());
        let button = PanelButton::symbol_default(symbol, label, &action, mtm);
        self.ivars().action_stack.addArrangedSubview(&button);
        button
    }

    pub fn set_status(&self, text: &str) {
        let status_label = &self.ivars().status_label;
        status_label.setStringValue(&ns_string(text));
        status_label.setHidden(text.is_empty());
        set_label(&**status_label, text);
    }

    pub fn use_review_bar_layout(&self) {
        self.ivars().label.setFont(Some(&PanelFont::system(12.5, weight_medium())));
        self.ivars().action_stack.setSpacing(3.0);
        self.invalidateIntrinsicContentSize();
    }

    /// Adjusts the gap after one action so the stack reads as groups.
    pub fn set_action_spacing(&self, spacing: CGFloat, after: &NSButton) {
        self.ivars().action_stack.setCustomSpacing_afterView(spacing, after);
        self.invalidateIntrinsicContentSize();
    }

    /// `applyStyle()`, dispatched dynamically so a subclass's override runs.
    pub fn apply_style(&self) {
        let _: () = unsafe { msg_send![self, applyStyle] };
    }

    /// The base implementation, which an overriding subclass calls as
    /// `super.applyStyle()`.
    pub fn base_apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.label.setTextColor(Some(&style_sheet.text));
        ivars.status_label.setTextColor(Some(&style_sheet.text_faint));
        if let Some(layer) = ivars.status_label.layer() {
            let alpha = if style_sheet.increase_contrast { 0.11 } else { 0.06 };
            layer.setBackgroundColor(Some(&cg(&style_sheet.text.colorWithAlphaComponent(alpha))));
        }
        // Dismiss is the weakest action on the bar.
        if let Some(close) = ivars.close_button.borrow().as_ref() {
            close.setContentTintColor(Some(&style_sheet.text_faint));
        }
        self.setNeedsDisplay(true);
    }

    pub fn label_for_testing(&self) -> Retained<NSTextField> {
        self.ivars().label.clone()
    }

    pub fn action_stack_for_testing(&self) -> Retained<NSStackView> {
        self.ivars().action_stack.clone()
    }
}

// MARK: - PanelSegmentedControl

pub struct PanelSegmentedControlIvars {
    items: Vec<String>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    selected_index: Cell<isize>,
    on_change: RefCell<Option<Rc<dyn Fn(isize)>>>,
    background_layer: Retained<CALayer>,
    thumb_layer: Retained<CALayer>,
    text_layers: RefCell<Vec<Retained<CATextLayer>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    is_pointer_inside: Cell<bool>,
    hovered_index: Cell<Option<isize>>,
    is_scrubbing: Cell<bool>,
    thumb_idle_color: RefCell<Retained<NSColor>>,
    disabled_indices: RefCell<BTreeSet<isize>>,
    natural_widths: Vec<CGFloat>,
}

define_class!(
    /// A quiet, animated two-or-more-way filter (§11.4).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelSegmentedControl"]
    #[ivars = PanelSegmentedControlIvars]
    pub struct PanelSegmentedControl;

    unsafe impl NSObjectProtocol for PanelSegmentedControl {}

    impl PanelSegmentedControl {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(self.ivars().natural_widths.iter().fold(0.0, |a, b| a + b), Self::CONTROL_HEIGHT)
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let widths = self.laid_out_widths();
            let ivars = self.ivars();
            let bounds = self.bounds();
            ivars.background_layer.setFrame(bounds);
            ivars.background_layer.setCornerRadius(Self::track_radius(bounds.height()));
            ivars.thumb_layer.setFrame(self.thumb_frame());
            ivars.thumb_layer.setCornerRadius(Self::track_radius(bounds.height()) - 2.0);
            // A `CATextLayer` draws its first line from the *top* of its
            // bounds; give the layer exactly one line and centre that.
            let font = Self::label_font();
            let line_height = (font.ascender() - font.descender()).ceil();
            let mut x: CGFloat = 0.0;
            for (index, text_layer) in ivars.text_layers.borrow().iter().enumerate() {
                let width = widths.get(index).copied().unwrap_or(0.0);
                text_layer.setFrame(rect(x, ((bounds.height() - line_height) / 2.0).round(), width, line_height));
                x += width;
            }
            CATransaction::commit();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect
                    | NSTrackingAreaOptions::MouseEnteredAndExited,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, event: &NSEvent) {
            self.ivars().is_pointer_inside.set(true);
            let x = self.convertPoint_fromView(event.locationInWindow(), None).x;
            self.ivars().hovered_index.set(Some(self.segment_at(x)));
            self.apply_selection_colors();
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_pointer_inside.set(false);
            self.ivars().hovered_index.set(None);
            self.apply_selection_colors();
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.ivars().is_scrubbing.set(true);
            self.set_thumb_pressed(true);
            self.select_if_changed(self.convertPoint_fromView(event.locationInWindow(), None).x);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            let x = self.convertPoint_fromView(event.locationInWindow(), None).x;
            self.ivars().hovered_index.set(Some(self.segment_at(x)));
            self.apply_selection_colors();
            self.select_if_changed(self.convertPoint_fromView(event.locationInWindow(), None).x);
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            self.ivars().is_scrubbing.set(false);
            self.set_thumb_pressed(false);
            self.ivars().hovered_index.set(None);
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            self.ivars().is_pointer_inside.set(self.bounds().contains_point(point));
            self.select_if_changed(self.convertPoint_fromView(event.locationInWindow(), None).x);
            self.apply_selection_colors();
        }

        #[unsafe(method(accessibilityPerformIncrement))]
        fn __accessibility_perform_increment(&self) -> bool {
            self.step(1)
        }

        #[unsafe(method(accessibilityPerformDecrement))]
        fn __accessibility_perform_decrement(&self) -> bool {
            self.step(-1)
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            NSApplication::sharedApplication(self.mtm()).isFullKeyboardAccessEnabled()
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn __focus_ring_mask_bounds(&self) -> NSRect {
            self.bounds()
        }

        #[unsafe(method(drawFocusRingMask))]
        fn __draw_focus_ring_mask(&self) {
            let radius = Self::track_radius(self.bounds().height());
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(self.bounds(), radius, radius).fill();
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            match KeyBinding::key_for_event(event).as_deref() {
                Some("left") | Some("up") => {
                    if self.step(-1) {
                        return;
                    }
                }
                Some("right") | Some("down") | Some("space") => {
                    if self.step(1) {
                        return;
                    }
                }
                _ => {}
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            let scale = backing_scale(self);
            for text_layer in self.ivars().text_layers.borrow().iter() {
                text_layer.setContentsScale(scale);
            }
        }
    }
);

/// `window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2`.
pub fn backing_scale(view: &NSView) -> CGFloat {
    if let Some(window) = view.window() {
        return window.backingScaleFactor();
    }
    objc2_app_kit::NSScreen::mainScreen(view.mtm()).map_or(2.0, |screen| screen.backingScaleFactor())
}

impl PanelSegmentedControl {
    pub const CONTROL_HEIGHT: CGFloat = 26.0;
    const MINIMUM_SEGMENT_WIDTH: CGFloat = 46.0;
    const LABEL_PADDING: CGFloat = 20.0;

    fn label_font() -> Retained<NSFont> {
        PanelFont::system(11.5, weight_semibold())
    }

    /// The track's radius follows its height.
    fn track_radius(height: CGFloat) -> CGFloat {
        smax(5.0, smin(8.0, PanelMetrics::capsule_radius(height) - 5.0))
    }

    /// `init(items:selectedIndex:styleSheet:)`; Swift's default index is 0.
    pub fn new(
        items: &[&str],
        selected_index: isize,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<PanelSegmentedControl> {
        let items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
        let count = items.len() as isize;
        let selected = selected_index.max(0).min(0.max(count - 1));
        let label_font = Self::label_font();
        let natural_widths: Vec<CGFloat> = items
            .iter()
            .map(|item| {
                let attributes = upleft_render::appkit_compat::attributes_dictionary(&[(
                    upleft_render::appkit_compat::keys::font(),
                    &label_font,
                )]);
                let width = unsafe {
                    objc2_app_kit::NSStringDrawing::sizeWithAttributes(&*ns_string(item), Some(&attributes)).width
                };
                smax(Self::MINIMUM_SEGMENT_WIDTH, width.ceil() + Self::LABEL_PADDING)
            })
            .collect();
        let this = Self::alloc(mtm).set_ivars(PanelSegmentedControlIvars {
            items,
            style_sheet: RefCell::new(style_sheet),
            selected_index: Cell::new(selected),
            on_change: RefCell::new(None),
            background_layer: CALayer::new(),
            thumb_layer: CALayer::new(),
            text_layers: RefCell::new(Vec::new()),
            tracking_area: RefCell::new(None),
            is_pointer_inside: Cell::new(false),
            hovered_index: Cell::new(None),
            is_scrubbing: Cell::new(false),
            thumb_idle_color: RefCell::new(NSColor::clearColor()),
            disabled_indices: RefCell::new(BTreeSet::new()),
            natural_widths,
        });
        let this: Retained<PanelSegmentedControl> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        let ivars = this.ivars();
        let layer = this.layer();

        ivars.background_layer.setCornerRadius(PanelMetrics::capsule_radius(Self::CONTROL_HEIGHT));
        if let Some(layer) = &layer {
            layer.addSublayer(&ivars.background_layer);
        }

        ivars.thumb_layer.setCornerRadius(PanelMetrics::capsule_radius(Self::CONTROL_HEIGHT) - 2.0);
        if let Some(layer) = &layer {
            layer.addSublayer(&ivars.thumb_layer);
        }

        for item in &ivars.items {
            let text_layer = CATextLayer::new();
            text_layer.setAlignmentMode(unsafe { kCAAlignmentCenter });
            text_layer.setContentsScale(backing_scale(&this));
            unsafe { text_layer.setString(Some(&ns_string(item))) };
            if let Some(layer) = &layer {
                layer.addSublayer(&text_layer);
            }
            ivars.text_layers.borrow_mut().push(text_layer);
        }

        this.apply_style();
        this.setAccessibilityElement(true);
        set_role(&*this, role::radio_group());
        set_label(&*this, &ivars.items.join("/"));
        // Swift reads the *parameter* here (it shadows the clamped
        // property), so an out-of-range index traps; so does this.
        set_value(&*this, &ivars.items[selected_index as usize]);
        this
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn selected_index(&self) -> isize {
        self.ivars().selected_index.get()
    }

    pub fn set_on_change(&self, handler: Option<Rc<dyn Fn(isize)>>) {
        *self.ivars().on_change.borrow_mut() = handler;
    }

    fn laid_out_widths(&self) -> Vec<CGFloat> {
        let natural_widths = &self.ivars().natural_widths;
        let natural = natural_widths.iter().fold(0.0, |a, b| a + b);
        let width = self.bounds().width();
        if !(natural > 0.0 && width > 0.0) {
            return natural_widths.clone();
        }
        let scale = width / natural;
        natural_widths.iter().map(|w| w * scale).collect()
    }

    fn segment_origin(&self, index: isize) -> CGFloat {
        self.laid_out_widths().iter().take(index.max(0) as usize).fold(0.0, |a, b| a + b)
    }

    pub fn set_enabled(&self, enabled: bool, segment: isize) {
        if !(segment >= 0 && (segment as usize) < self.ivars().items.len()) {
            return;
        }
        if enabled {
            self.ivars().disabled_indices.borrow_mut().remove(&segment);
        } else {
            self.ivars().disabled_indices.borrow_mut().insert(segment);
        }
        self.apply_selection_colors();
    }

    pub fn is_enabled(&self, segment: isize) -> bool {
        !self.ivars().disabled_indices.borrow().contains(&segment)
    }

    /// `setSelectedIndex(_:animated:)`; Swift's default is animated.
    pub fn set_selected_index(&self, index: isize, animated: bool) {
        let count = self.ivars().items.len() as isize;
        let normalized = index.max(0).min(0.max(count - 1));
        if normalized == self.ivars().selected_index.get() {
            return;
        }
        self.ivars().selected_index.set(normalized);
        set_value(self, &self.ivars().items[normalized as usize]);
        self.update_thumb(animated);
        self.apply_selection_colors();
    }

    fn thumb_frame(&self) -> NSRect {
        let widths = self.laid_out_widths();
        let selected = self.ivars().selected_index.get();
        rect(
            self.segment_origin(selected) + 1.0,
            1.0,
            smax(0.0, widths.get(selected as usize).copied().unwrap_or(0.0) - 2.0),
            self.bounds().height() - 2.0,
        )
    }

    fn update_thumb(&self, animated: bool) {
        let target = self.thumb_frame();
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        if !(animated && !reduce_motion && self.window().is_some()) {
            without_actions(|| self.ivars().thumb_layer.setFrame(target));
            return;
        }
        CATransaction::begin();
        CATransaction::setAnimationDuration(motion::STANDARD);
        CATransaction::setAnimationTimingFunction(Some(&motion::timing(Curve::Structural)));
        self.ivars().thumb_layer.setFrame(target);
        CATransaction::commit();
    }

    fn apply_selection_colors(&self) {
        let ivars = self.ivars();
        let selected_index = ivars.selected_index.get();
        let style_sheet = ivars.style_sheet.borrow().clone();
        for (index, text_layer) in ivars.text_layers.borrow().iter().enumerate() {
            let index = index as isize;
            let selected = index == selected_index;
            let font = if selected { Self::label_font() } else { PanelFont::system(11.5, weight_medium()) };
            // SAFETY: `NSFont` is toll-free bridged with `CTFont`, which is a
            // valid `CATextLayer.font`.
            unsafe { text_layer.setFont(Some(&*(Retained::as_ptr(&font) as *const objc2_core_foundation::CFType))) };
            text_layer.setFontSize(Self::label_font().pointSize());
            let alpha: CGFloat = if ivars.disabled_indices.borrow().contains(&index) {
                if style_sheet.increase_contrast { 0.45 } else { 0.30 }
            } else if selected {
                1.0
            } else {
                self.dimmed_alpha(index)
            };
            let color = if selected { style_sheet.text.clone() } else { style_sheet.text_secondary.clone() };
            text_layer.setForegroundColor(Some(&cg(&color.colorWithAlphaComponent(alpha))));
        }
    }

    fn dimmed_alpha(&self, index: isize) -> CGFloat {
        let contrast = self.ivars().style_sheet.borrow().increase_contrast;
        if self.ivars().is_pointer_inside.get() && self.ivars().hovered_index.get() == Some(index) {
            return if contrast { 0.92 } else { 0.78 };
        }
        if contrast { 0.82 } else { 0.62 }
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        ivars.background_layer.setBackgroundColor(Some(&cg(&style_sheet.text.panel_alpha(0.06, contrast))));
        *ivars.thumb_idle_color.borrow_mut() = style_sheet.text.panel_alpha(0.13, contrast);
        ivars.thumb_layer.setBackgroundColor(Some(&cg(&ivars.thumb_idle_color.borrow())));
        self.apply_selection_colors();
        self.setNeedsDisplay(true);
    }

    /// The thumb recesses while the pointer holds it and springs back past
    /// full size on release.
    fn set_thumb_pressed(&self, pressed: bool) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        let pressed_color = style_sheet.text.panel_alpha(if contrast { 0.22 } else { 0.18 }, contrast);
        let thumb = &ivars.thumb_layer;
        if !(!style_sheet.reduce_motion && self.window().is_some()) {
            without_actions(|| {
                thumb.setTransform(unsafe { CATransform3DIdentity });
                thumb.setBackgroundColor(Some(&cg(&ivars.thumb_idle_color.borrow())));
            });
            return;
        }

        if pressed {
            let press = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
            set_transform_values(&press, presentation_transform(thumb), CATransform3D::new_scale(0.92, 0.92, 1.0));
            press.setDuration(ToolbarChromePolicy::PRESS_IN_DURATION);
            press.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
            let color = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("backgroundColor")));
            set_color_values(&color, thumb.backgroundColor().as_deref(), &cg(&pressed_color));
            color.setDuration(ToolbarChromePolicy::PRESS_IN_DURATION);
            thumb.removeAnimationForKey(&NSString::from_str("thumb-settle"));
            thumb.addAnimation_forKey(&press, Some(&NSString::from_str("thumb-press")));
            thumb.addAnimation_forKey(&color, Some(&NSString::from_str("thumb-press-color")));
            thumb.setTransform(CATransform3D::new_scale(0.92, 0.92, 1.0));
            thumb.setBackgroundColor(Some(&cg(&pressed_color)));
        } else {
            let from = presentation_transform(thumb);
            let settle = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
            let values = NSArray::from_retained_slice(&[
                transform_value(from),
                transform_value(CATransform3D::new_scale(1.06, 1.06, 1.0)),
                transform_value(unsafe { CATransform3DIdentity }),
            ]);
            unsafe { settle.setValues(Some(Retained::cast_unchecked::<NSArray>(values).as_ref())) };
            settle.setKeyTimes(Some(&NSArray::from_retained_slice(&[
                NSNumber::new_f64(0.0),
                NSNumber::new_f64(0.45),
                NSNumber::new_f64(1.0),
            ])));
            settle.setDuration(motion::STANDARD);
            settle.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
                motion::timing(Curve::Decelerate),
                motion::timing(Curve::EaseOut),
            ])));
            let color = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("backgroundColor")));
            set_color_values(&color, thumb.backgroundColor().as_deref(), &cg(&ivars.thumb_idle_color.borrow()));
            color.setDuration(motion::QUICK);
            thumb.removeAnimationForKey(&NSString::from_str("thumb-press"));
            thumb.removeAnimationForKey(&NSString::from_str("thumb-press-color"));
            thumb.addAnimation_forKey(&settle, Some(&NSString::from_str("thumb-settle")));
            thumb.addAnimation_forKey(&color, Some(&NSString::from_str("thumb-color-settle")));
            thumb.setTransform(unsafe { CATransform3DIdentity });
            thumb.setBackgroundColor(Some(&cg(&ivars.thumb_idle_color.borrow())));
        }
    }

    fn segment_at(&self, x: CGFloat) -> isize {
        if !(self.bounds().width() > 0.0) {
            return 0;
        }
        let mut offset: CGFloat = 0.0;
        for (index, width) in self.laid_out_widths().iter().enumerate() {
            offset += width;
            if x < offset {
                return index as isize;
            }
        }
        0.max(self.ivars().items.len() as isize - 1)
    }

    fn select_if_changed(&self, x: CGFloat) {
        let index = self.segment_at(x);
        if self.ivars().disabled_indices.borrow().contains(&index) {
            return;
        }
        let previous = self.ivars().selected_index.get();
        self.set_selected_index(index, true);
        if self.ivars().selected_index.get() != previous {
            perform_alignment_haptic();
            let handler = self.ivars().on_change.borrow().clone();
            if let Some(handler) = handler {
                handler(self.ivars().selected_index.get());
            }
        }
    }

    fn step(&self, delta: isize) -> bool {
        let count = self.ivars().items.len() as isize;
        let mut candidate = self.ivars().selected_index.get() + delta;
        while candidate >= 0 && candidate < count && self.ivars().disabled_indices.borrow().contains(&candidate) {
            candidate += delta;
        }
        if !(candidate >= 0 && candidate < count) {
            return false;
        }
        self.set_selected_index(candidate, true);
        let handler = self.ivars().on_change.borrow().clone();
        if let Some(handler) = handler {
            handler(self.ivars().selected_index.get());
        }
        true
    }

    pub fn text_layers_for_testing(&self) -> Vec<Retained<CATextLayer>> {
        self.ivars().text_layers.borrow().clone()
    }

    pub fn thumb_layer_for_testing(&self) -> Retained<CALayer> {
        self.ivars().thumb_layer.clone()
    }
}

/// `NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .now)`.
pub fn perform_alignment_haptic() {
    let performer = NSHapticFeedbackManager::defaultPerformer();
    performer.performFeedbackPattern_performanceTime(NSHapticFeedbackPattern::Alignment, NSHapticFeedbackPerformanceTime::Now);
}

// MARK: - PanelProgressBar

pub struct PanelProgressBarIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    fraction: Cell<CGFloat>,
    track_layer: Retained<CALayer>,
    fill_layer: Retained<CALayer>,
}

define_class!(
    /// A thin, animated completion bar used by the task panel header (§8.5).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelProgressBar"]
    #[ivars = PanelProgressBarIvars]
    pub struct PanelProgressBar;

    unsafe impl NSObjectProtocol for PanelProgressBar {}

    impl PanelProgressBar {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(unsafe { NSViewNoIntrinsicMetric }, 4.0)
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let ivars = self.ivars();
            let bounds = self.bounds();
            ivars.track_layer.setFrame(bounds);
            ivars.track_layer.setCornerRadius(bounds.height() / 2.0);
            ivars.fill_layer.setCornerRadius(bounds.height() / 2.0);
            self.place_fill(false);
            CATransaction::commit();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }
);

impl PanelProgressBar {
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<PanelProgressBar> {
        let this = Self::alloc(mtm).set_ivars(PanelProgressBarIvars {
            style_sheet: RefCell::new(style_sheet),
            fraction: Cell::new(0.0),
            track_layer: CALayer::new(),
            fill_layer: CALayer::new(),
        });
        let this: Retained<PanelProgressBar> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.addSublayer(&this.ivars().track_layer);
            layer.addSublayer(&this.ivars().fill_layer);
        }
        this.apply_style();
        this
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn fraction(&self) -> CGFloat {
        self.ivars().fraction.get()
    }

    pub fn set_fraction(&self, fraction: CGFloat) {
        let old_value = self.ivars().fraction.get();
        self.ivars().fraction.set(fraction);
        let clamped = smin(smax(fraction, 0.0), 1.0);
        if clamped == old_value {
            return;
        }
        self.ivars().fraction.set(clamped);
        self.place_fill(self.window().is_some());
    }

    fn place_fill(&self, animated: bool) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let fraction = ivars.fraction.get();
        let width = bounds.width() * fraction;
        let minimum = if fraction > 0.0 { bounds.height() } else { 0.0 };
        let target = rect(0.0, 0.0, smax(minimum, width), bounds.height());
        ivars.fill_layer.setHidden(target.width() == 0.0);
        let reduce_motion = ivars.style_sheet.borrow().reduce_motion;
        if !(animated && !reduce_motion && self.window().is_some()) {
            without_actions(|| ivars.fill_layer.setFrame(target));
            return;
        }
        CATransaction::begin();
        CATransaction::setAnimationDuration(motion::DELIBERATE);
        CATransaction::setAnimationTimingFunction(Some(&motion::timing(Curve::Structural)));
        ivars.fill_layer.setFrame(target);
        CATransaction::commit();
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let contrast = style_sheet.increase_contrast;
        ivars.track_layer.setBackgroundColor(Some(&cg(&style_sheet.text.panel_alpha(0.13, contrast))));
        ivars.fill_layer.setBackgroundColor(Some(&cg(&style_sheet.accent)));
        self.setNeedsDisplay(true);
    }
}

// MARK: - PanelCheckbox

/// `PanelCheckbox.CheckState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Off,
    On,
    Mixed,
}

/// `PanelCheckbox.Geometry`: the document's 20pt box, as ratios.
pub struct CheckboxGeometry;

impl CheckboxGeometry {
    pub const PANEL_SIDE: CGFloat = 16.0;
    pub const HIT_INSET: CGFloat = 6.0;
    pub const CORNER_RATIO: CGFloat = render_metrics::TASK_BOX_CORNER_RATIO;
    pub const OPEN_STROKE_RATIO: CGFloat = render_metrics::TASK_BOX_STROKE_RATIO;
    pub const CHECK_STROKE_RATIO: CGFloat = render_metrics::TASK_TICK_STROKE_RATIO;
    pub const TICK: [NSPoint; 3] = render_metrics::TASK_TICK;
    pub const DASH_INSET: CGFloat = 0.28;
}

pub struct PanelCheckboxIvars {
    on_toggle: RefCell<Option<Rc<dyn Fn()>>>,
    on_press_change: RefCell<Option<Rc<dyn Fn(bool)>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    border_layer: Retained<CAShapeLayer>,
    fill_layer: Retained<CAShapeLayer>,
    check_layer: Retained<CAShapeLayer>,
    ripple_layer: Retained<CAShapeLayer>,
    dash_layer: Retained<CAShapeLayer>,
    state: Cell<CheckState>,
    is_hovering: Cell<bool>,
    is_pressed: Cell<bool>,
    side: CGFloat,
    corner_ratio: CGFloat,
}

define_class!(
    /// The one checkbox in the app, drawn from the document's geometry (§8.5).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelCheckbox"]
    #[ivars = PanelCheckboxIvars]
    pub struct PanelCheckbox;

    unsafe impl NSObjectProtocol for PanelCheckbox {}

    impl PanelCheckbox {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(self.ivars().side, self.ivars().side)
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test(point)
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            NSApplication::sharedApplication(self.mtm()).isFullKeyboardAccessEnabled()
        }

        #[unsafe(method(canBecomeKeyView))]
        fn __can_become_key_view(&self) -> bool {
            NSApplication::sharedApplication(self.mtm()).isFullKeyboardAccessEnabled()
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn __focus_ring_mask_bounds(&self) -> NSRect {
            self.bounds().inset_by(-2.5, -2.5)
        }

        #[unsafe(method(drawFocusRingMask))]
        fn __draw_focus_ring_mask(&self) {
            let r = self.bounds().inset_by(-2.5, -2.5);
            let radius = self.ivars().side * self.ivars().corner_ratio + 2.5;
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, radius, radius).fill();
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

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            if KeyBinding::key_for_event(event).as_deref() != Some("space") {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
                return;
            }
            self.perform_toggle();
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let ivars = self.ivars();
            let bounds = self.bounds();
            let side = smin(bounds.width(), bounds.height());
            let inset = side * CheckboxGeometry::OPEN_STROKE_RATIO / 2.0;
            let r = bounds.inset_by(inset, inset);
            let radius = side * ivars.corner_ratio;
            // SAFETY: a null transform is allowed.
            let box_path = unsafe { CGPath::with_rounded_rect(r, radius, radius, std::ptr::null()) };

            for layer in [&ivars.fill_layer, &ivars.border_layer, &ivars.check_layer, &ivars.dash_layer, &ivars.ripple_layer] {
                layer.setFrame(bounds);
            }
            ivars.border_layer.setPath(Some(&box_path));
            ivars.fill_layer.setPath(Some(&box_path));
            ivars.fill_layer.setCornerRadius(radius);

            ivars.check_layer.setPath(Some(&Self::tick_path(side)));
            ivars.check_layer.setLineWidth(side * CheckboxGeometry::CHECK_STROKE_RATIO);
            ivars.check_layer.setLineCap(unsafe { kCALineCapRound });
            ivars.check_layer.setLineJoin(unsafe { kCALineJoinRound });
            ivars.check_layer.setStrokeStart(0.0);
            ivars.check_layer.setStrokeEnd(if ivars.state.get() == CheckState::On { 1.0 } else { 0.0 });

            ivars.dash_layer.setPath(Some(&Self::dash_path(side)));
            ivars.dash_layer.setLineWidth(side * CheckboxGeometry::CHECK_STROKE_RATIO);
            ivars.dash_layer.setLineCap(unsafe { kCALineCapRound });

            ivars.ripple_layer.setPath(Some(&box_path));
            ivars.ripple_layer.setLineWidth(smax(1.0, side * CheckboxGeometry::CHECK_STROKE_RATIO * 0.9));
            ivars.ripple_layer.setStrokeColor(Some(&cg(&ivars.style_sheet.borrow().accent)));
            CATransaction::commit();
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            self.ivars().is_pressed.set(true);
            self.update_transform(true);
            self.notify_press_change(true);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            let inside = self.hit_bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            if inside == self.ivars().is_pressed.get() {
                return;
            }
            self.ivars().is_pressed.set(inside);
            self.update_transform(true);
            self.notify_press_change(self.ivars().is_pressed.get());
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            let inside = self.hit_bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            self.ivars().is_pressed.set(false);
            self.update_transform(true);
            self.notify_press_change(false);
            if inside {
                self.perform_toggle();
            }
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            // SAFETY: the owner is the view itself.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.hit_bounds(),
                    NSTrackingAreaOptions::ActiveInKeyWindow | NSTrackingAreaOptions::MouseEnteredAndExited,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            if self.ivars().is_hovering.get() {
                return;
            }
            self.ivars().is_hovering.set(true);
            self.apply_visual(true);
            self.update_transform(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            if !self.ivars().is_hovering.get() {
                return;
            }
            self.ivars().is_hovering.set(false);
            self.apply_visual(true);
            self.update_transform(true);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn __accessibility_perform_press(&self) -> bool {
            self.perform_toggle();
            true
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_visual(false);
        }
    }
);

impl PanelCheckbox {
    /// `init(side:cornerRatio:)`; Swift's defaults are
    /// `Geometry.panelSide` and `Geometry.cornerRatio`.
    pub fn new(side: CGFloat, corner_ratio: CGFloat, mtm: MainThreadMarker) -> Retained<PanelCheckbox> {
        let this = Self::alloc(mtm).set_ivars(PanelCheckboxIvars {
            on_toggle: RefCell::new(None),
            on_press_change: RefCell::new(None),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
            border_layer: CAShapeLayer::new(),
            fill_layer: CAShapeLayer::new(),
            check_layer: CAShapeLayer::new(),
            ripple_layer: CAShapeLayer::new(),
            dash_layer: CAShapeLayer::new(),
            state: Cell::new(CheckState::Off),
            is_hovering: Cell::new(false),
            is_pressed: Cell::new(false),
            side,
            corner_ratio,
        });
        let this: Retained<PanelCheckbox> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, side, side)] };
        this.setWantsLayer(true);
        let ivars = this.ivars();
        for layer in [&ivars.fill_layer, &ivars.border_layer, &ivars.ripple_layer, &ivars.check_layer, &ivars.dash_layer] {
            layer.setFillColor(Some(&cg(&NSColor::clearColor())));
            null_actions(layer, &["position", "bounds"]);
            if let Some(own) = this.layer() {
                own.addSublayer(layer);
            }
        }
        ivars.ripple_layer.setOpacity(0.0);
        this.setFocusRingType(NSFocusRingType::Default);
        this.setAccessibilityElement(true);
        set_role(&*this, role::check_box());
        set_value(&*this, "Unchecked");
        this
    }

    /// `PanelCheckbox()` with the Swift defaults.
    pub fn new_default(mtm: MainThreadMarker) -> Retained<PanelCheckbox> {
        Self::new(CheckboxGeometry::PANEL_SIDE, CheckboxGeometry::CORNER_RATIO, mtm)
    }

    pub fn set_on_toggle(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_toggle.borrow_mut() = handler;
    }

    pub fn set_on_press_change(&self, handler: Option<Rc<dyn Fn(bool)>>) {
        *self.ivars().on_press_change.borrow_mut() = handler;
    }

    fn notify_press_change(&self, pressed: bool) {
        let handler = self.ivars().on_press_change.borrow().clone();
        if let Some(handler) = handler {
            handler(pressed);
        }
    }

    pub fn state(&self) -> CheckState {
        self.ivars().state.get()
    }

    fn is_filled(&self) -> bool {
        self.ivars().state.get() != CheckState::Off
    }

    /// A view whose frame is the drawn box can still claim the slack around
    /// it; `hitTest` reports the point in the superview's space.
    fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let superview = superview(self)?;
        if self.hit_bounds().contains_point(self.convertPoint_fromView(point, Some(&superview))) {
            Some(Retained::into_super(self.retain()))
        } else {
            None
        }
    }

    /// The rect the pointer may press, which is larger than the box it draws.
    pub fn hit_bounds(&self) -> NSRect {
        self.bounds().inset_by(-CheckboxGeometry::HIT_INSET, -CheckboxGeometry::HIT_INSET)
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_visual(false);
    }

    pub fn set_checked(&self, checked: bool, animated: bool) {
        self.set_state(if checked { CheckState::On } else { CheckState::Off }, animated);
    }

    pub fn set_state(&self, new_state: CheckState, animated: bool) {
        if new_state == self.ivars().state.get() {
            return;
        }
        self.ivars().state.set(new_state);
        self.apply_visual(animated);
    }

    fn dash_path(side: CGFloat) -> Retained<CGPath> {
        let path = NSBezierPath::bezierPath();
        path.moveToPoint(NSPoint::new(side * CheckboxGeometry::DASH_INSET, side / 2.0));
        path.lineToPoint(NSPoint::new(side * (1.0 - CheckboxGeometry::DASH_INSET), side / 2.0));
        path.CGPath()
    }

    /// The renderer's tick, scaled.
    fn tick_path(side: CGFloat) -> Retained<CGPath> {
        let path = NSBezierPath::bezierPath();
        for (index, point) in CheckboxGeometry::TICK.iter().enumerate() {
            let scaled = NSPoint::new(point.x * side, point.y * side);
            if index == 0 {
                path.moveToPoint(scaled);
            } else {
                path.lineToPoint(scaled);
            }
        }
        path.CGPath()
    }

    /// The box's outline.
    fn ring_color(&self) -> Retained<NSColor> {
        let style_sheet = self.ivars().style_sheet.borrow().clone();
        if self.ivars().is_hovering.get() && !self.is_filled() {
            let contrast = style_sheet.increase_contrast;
            return style_sheet.accent.panel_alpha(if contrast { 1.0 } else { 0.85 }, false);
        }
        style_sheet.task_ring_color(self.is_filled())
    }

    fn apply_visual(&self, animated: bool) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let side = smin(bounds.width(), bounds.height());
        let style_sheet = ivars.style_sheet.borrow().clone();
        let state = ivars.state.get();
        let target_border = self.ring_color();
        let target_fill_alpha: f32 = if self.is_filled() {
            1.0
        } else if ivars.is_hovering.get() {
            0.09
        } else {
            0.0
        };
        let check_end: CGFloat = if state == CheckState::On { 1.0 } else { 0.0 };

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        ivars.dash_layer.setStrokeColor(Some(&cg(&style_sheet.task_tick_color())));
        ivars.check_layer.setStrokeColor(Some(&cg(&style_sheet.task_tick_color())));
        ivars.fill_layer.setBackgroundColor(Some(&cg(&style_sheet.task_field_color())));
        CATransaction::commit();

        if !(animated && !style_sheet.reduce_motion && self.window().is_some()) {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            for layer in [&ivars.fill_layer, &ivars.check_layer, &ivars.border_layer, &ivars.dash_layer] {
                layer.removeAllAnimations();
            }
            ivars.border_layer.setStrokeColor(Some(&cg(&target_border)));
            ivars.border_layer.setLineWidth(side * CheckboxGeometry::OPEN_STROKE_RATIO);
            ivars.fill_layer.setOpacity(target_fill_alpha);
            ivars.fill_layer.setTransform(unsafe { CATransform3DIdentity });
            ivars.check_layer.setStrokeEnd(check_end);
            ivars.check_layer.setOpacity(check_end as f32);
            ivars.dash_layer.setOpacity(if state == CheckState::Mixed { 1.0 } else { 0.0 });
            CATransaction::commit();
            return;
        }

        let dash_fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let dash_from = ivars
            .dash_layer
            .__presentation()
            .map(|layer| layer.opacity())
            .unwrap_or_else(|| ivars.dash_layer.opacity());
        set_number_values(&dash_fade, Some(dash_from as f64), if state == CheckState::Mixed { 1.0 } else { 0.0 });
        dash_fade.setDuration(motion::QUICK);
        ivars.dash_layer.removeAllAnimations();
        ivars.dash_layer.addAnimation_forKey(&dash_fade, Some(&NSString::from_str("dash-fade")));
        ivars.dash_layer.setOpacity(if state == CheckState::Mixed { 1.0 } else { 0.0 });

        let border = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeColor")));
        let border_from = ivars
            .border_layer
            .__presentation()
            .and_then(|layer| layer.strokeColor())
            .or_else(|| ivars.border_layer.strokeColor());
        set_color_values(&border, border_from.as_deref(), &cg(&target_border));
        border.setDuration(motion::QUICK);
        border.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
        ivars.border_layer.setStrokeColor(Some(&cg(&target_border)));
        ivars.border_layer.setLineWidth(side * CheckboxGeometry::OPEN_STROKE_RATIO);
        ivars.border_layer.addAnimation_forKey(&border, Some(&NSString::from_str("border")));

        if self.is_filled() {
            let pop = motion::pop(0.7, 1.07, motion::STANDARD, Some(CGVector::new(1.0, 0.0)), Some(bounds.height() / 2.0));
            let fade_in = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
            set_number_values(&fade_in, Some(0.0), 1.0);
            fade_in.setDuration(motion::QUICK);

            ivars.fill_layer.removeAllAnimations();
            ivars.fill_layer.addAnimation_forKey(&pop, Some(&NSString::from_str("fill-pop")));
            ivars.fill_layer.addAnimation_forKey(&fade_in, Some(&NSString::from_str("fill-fade")));
            ivars.fill_layer.setOpacity(1.0);

            ivars.check_layer.removeAllAnimations();
            if state == CheckState::On {
                let draw = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeEnd")));
                set_number_values(&draw, Some(0.0), 1.0);
                draw.setDuration(motion::STANDARD);
                draw.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
                ivars.check_layer.addAnimation_forKey(&draw, Some(&NSString::from_str("check-draw")));
            }
            ivars.check_layer.setStrokeEnd(check_end);
            ivars.check_layer.setOpacity(check_end as f32);
        } else {
            let retract = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeEnd")));
            let from = ivars.check_layer.__presentation().map(|layer| layer.strokeEnd()).unwrap_or(1.0);
            set_number_values(&retract, Some(from), 0.0);
            retract.setDuration(motion::QUICK);
            retract.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
            ivars.check_layer.removeAllAnimations();
            ivars.check_layer.addAnimation_forKey(&retract, Some(&NSString::from_str("check-retract")));
            ivars.check_layer.setStrokeEnd(0.0);
            ivars.check_layer.setOpacity(0.0);

            let fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
            let from = ivars
                .fill_layer
                .__presentation()
                .map(|layer| layer.opacity())
                .unwrap_or_else(|| ivars.fill_layer.opacity());
            set_number_values(&fade, Some(from as f64), target_fill_alpha as f64);
            fade.setDuration(motion::QUICK);
            ivars.fill_layer.removeAllAnimations();
            ivars.fill_layer.addAnimation_forKey(&fade, Some(&NSString::from_str("fill-fade-out")));
            ivars.fill_layer.setOpacity(target_fill_alpha);
        }
    }

    /// One toggle path for pointer, keyboard, and VoiceOver.
    pub fn perform_toggle(&self) {
        let ivars = self.ivars();
        ivars.state.set(if ivars.state.get() == CheckState::On { CheckState::Off } else { CheckState::On });
        self.apply_visual(true);
        let state = ivars.state.get();
        set_value(
            self,
            if state == CheckState::On {
                "Checked"
            } else if state == CheckState::Mixed {
                "Mixed"
            } else {
                "Unchecked"
            },
        );
        if self.is_filled() {
            perform_alignment_haptic();
            self.pulse_ripple();
        }
        let handler = ivars.on_toggle.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn pulse_ripple(&self) {
        let ivars = self.ivars();
        if !(self.window().is_some() && !ivars.style_sheet.borrow().reduce_motion) {
            ivars.ripple_layer.setOpacity(0.0);
            return;
        }
        ivars.ripple_layer.removeAllAnimations();
        let scale = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.scale")));
        set_number_values(&scale, Some(0.85), 1.6);
        scale.setDuration(motion::DELIBERATE);
        scale.setTimingFunction(Some(&motion::timing(Curve::Structural)));

        let fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        set_number_values(&fade, Some(0.5), 0.0);
        fade.setDuration(motion::DELIBERATE);
        fade.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));

        let group = CAAnimationGroup::animation();
        let animations: Retained<NSArray<CAAnimation>> = NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(scale)),
            Retained::into_super(Retained::into_super(fade)),
        ]);
        group.setAnimations(Some(&animations));
        group.setDuration(motion::DELIBERATE);
        ivars.ripple_layer.addAnimation_forKey(&group, Some(&NSString::from_str("ripple")));
        ivars.ripple_layer.setOpacity(0.0);
    }

    /// Hover lifts the box, a press pushes it back past its resting size.
    fn update_transform(&self, animated: bool) {
        let ivars = self.ivars();
        let scale: CGFloat = if ivars.is_pressed.get() {
            0.9
        } else if ivars.is_hovering.get() {
            1.06
        } else {
            1.0
        };
        let target = CATransform3D::new_scale(scale, scale, 1.0);
        let Some(layer) = self.layer() else { return };
        if !(animated && !ivars.style_sheet.borrow().reduce_motion) {
            layer.removeAnimationForKey(&NSString::from_str("press"));
            layer.setTransform(target);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        set_transform_values(&animation, presentation_transform(&layer), target);
        animation.setDuration(if ivars.is_pressed.get() {
            ToolbarChromePolicy::PRESS_IN_DURATION
        } else {
            ToolbarChromePolicy::PRESS_OUT_DURATION
        });
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        layer.addAnimation_forKey(&animation, Some(&NSString::from_str("press")));
        layer.setTransform(target);
    }
}

// MARK: - PanelSelectionRowView

pub struct PanelSelectionRowViewIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    /// Row selection drawn in the panel's own language instead of the system
    /// accent (§11.4).
    // SAFETY: `initWithFrame:` sets the ivars, so every AppKit path that
    // creates the row view creates a valid one.
    #[unsafe(super(NSTableRowView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelSelectionRowView"]
    #[ivars = PanelSelectionRowViewIvars]
    pub struct PanelSelectionRowView;

    unsafe impl NSObjectProtocol for PanelSelectionRowView {}

    impl PanelSelectionRowView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("PanelSelectionRowView is created on the main thread");
            let this = this.set_ivars(PanelSelectionRowViewIvars {
                style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
            });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        #[unsafe(method(drawBackgroundInRect:))]
        fn __draw_background_in_rect(&self, _dirty_rect: NSRect) {
            // Transparent: the panel backdrop shows through the table.
        }

        #[unsafe(method(drawSelectionInRect:))]
        fn __draw_selection_in_rect(&self, _dirty_rect: NSRect) {
            let style_sheet = self.ivars().style_sheet.borrow().clone();
            let contrast = style_sheet.increase_contrast;
            style_sheet.selection.panel_alpha(if contrast { 1.0 } else { 0.85 }, false).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                PanelMetrics::row_surface(self.bounds()),
                PanelMetrics::ROW_SURFACE_RADIUS,
                PanelMetrics::ROW_SURFACE_RADIUS,
            )
            .fill();
        }
    }
);

impl PanelSelectionRowView {
    /// `PanelSelectionRowView()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<PanelSelectionRowView> {
        unsafe { msg_send![PanelSelectionRowView::alloc(mtm), init] }
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.setNeedsDisplay(true);
    }
}

// MARK: - PanelEmptyStateView

pub struct PanelEmptyStateViewIvars {
    disc_layer: Retained<CALayer>,
    symbol_view: Retained<NSImageView>,
    title_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
}

define_class!(
    /// A two-line empty state with a tinted symbol disc (§11.4).
    // SAFETY: `initWithFrame:` sets the ivars.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PanelEmptyStateView"]
    #[ivars = PanelEmptyStateViewIvars]
    pub struct PanelEmptyStateView;

    unsafe impl NSObjectProtocol for PanelEmptyStateView {}

    impl PanelEmptyStateView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("PanelEmptyStateView is created on the main thread");
            let this = this.set_ivars(PanelEmptyStateViewIvars {
                disc_layer: CALayer::new(),
                symbol_view: NSImageView::new(mtm),
                title_label: label("", mtm),
                subtitle_label: wrapping_label("", mtm),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.finish_init();
            this
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let ivars = self.ivars();
            ivars.disc_layer.setFrame(ivars.symbol_view.frame().inset_by(-5.0, -5.0));
            ivars.disc_layer.setCornerRadius(ivars.disc_layer.bounds().width() / 2.0);
            CATransaction::commit();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.setNeedsDisplay(true);
        }
    }
);

impl PanelEmptyStateView {
    /// `PanelEmptyStateView()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<PanelEmptyStateView> {
        unsafe { msg_send![PanelEmptyStateView::alloc(mtm), initWithFrame: NSRect::ZERO] }
    }

    fn finish_init(&self) {
        self.setWantsLayer(true);
        let ivars = self.ivars();

        ivars.symbol_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.symbol_view.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        ivars.symbol_view.setContentTintColor(Some(&NSColor::clearColor()));
        self.addSubview(&ivars.symbol_view);

        ivars.disc_layer.setOpacity(0.0);
        if let Some(layer) = self.layer() {
            layer.addSublayer(&ivars.disc_layer);
        }

        ivars.title_label.setFont(Some(&PanelFont::system(12.5, weight_semibold())));
        ivars.title_label.setAlignment(NSTextAlignment::Center);
        ivars.title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&ivars.title_label);

        ivars.subtitle_label.setFont(Some(&PanelFont::secondary()));
        ivars.subtitle_label.setAlignment(NSTextAlignment::Center);
        ivars.subtitle_label.setMaximumNumberOfLines(3);
        ivars.subtitle_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&ivars.subtitle_label);

        ivars.title_label.setMaximumNumberOfLines(1);
        ivars.title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        ivars.title_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let symbol_view = &ivars.symbol_view;
        let title_label = &ivars.title_label;
        let subtitle_label = &ivars.subtitle_label;
        activate(&[
            symbol_view.topAnchor().constraintEqualToAnchor(&self.topAnchor()),
            symbol_view.centerXAnchor().constraintEqualToAnchor(&self.centerXAnchor()),
            symbol_view.widthAnchor().constraintEqualToConstant(36.0),
            symbol_view.heightAnchor().constraintEqualToConstant(36.0),
            title_label.topAnchor().constraintEqualToAnchor_constant(&symbol_view.bottomAnchor(), 10.0),
            title_label.centerXAnchor().constraintEqualToAnchor(&self.centerXAnchor()),
            title_label.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            title_label.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            subtitle_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 4.0),
            subtitle_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            subtitle_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            subtitle_label.centerXAnchor().constraintEqualToAnchor(&self.centerXAnchor()),
            subtitle_label.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);
    }

    pub fn title(&self) -> String {
        self.ivars().title_label.stringValue().to_string()
    }

    pub fn subtitle(&self) -> String {
        self.ivars().subtitle_label.stringValue().to_string()
    }

    pub fn configure(&self, symbol: &str, title: &str, subtitle: &str, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        let contrast = style_sheet.increase_contrast;
        ivars.disc_layer.setBackgroundColor(Some(&cg(&style_sheet.text_faint.panel_alpha(0.12, contrast))));
        ivars.disc_layer.setOpacity(1.0);

        let image = configured_symbol(symbol, Some(title), &symbol_configuration(17.0, weight_medium()));
        ivars.symbol_view.setImage(image.as_deref());
        ivars.symbol_view.setContentTintColor(Some(&style_sheet.text_secondary));

        ivars.title_label.setStringValue(&ns_string(title));
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        ivars.subtitle_label.setStringValue(&ns_string(subtitle));
        ivars.subtitle_label.setTextColor(Some(&style_sheet.text_secondary));
        set_role(self, role::group());
        let accessibility = if subtitle.is_empty() { title.to_owned() } else { format!("{title}: {subtitle}") };
        set_label(self, &accessibility);
    }

    /// Centre the state over `list` and hide it.  `vertical_bias` < 1 lifts
    /// the state toward the optical centre (Swift's default is 1.0).
    pub fn install(&self, panel: &NSView, list: &NSView, vertical_bias: CGFloat) {
        self.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.setHidden(true);
        panel.addSubview(self);
        let centre = unsafe {
            NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
                self,
                NSLayoutAttribute::CenterY,
                NSLayoutRelation::Equal,
                Some(list),
                NSLayoutAttribute::CenterY,
                vertical_bias,
                0.0,
            )
        };
        activate(&[
            self.leadingAnchor().constraintEqualToAnchor_constant(&panel.leadingAnchor(), PanelMetrics::INSET),
            self.trailingAnchor().constraintEqualToAnchor_constant(&panel.trailingAnchor(), -PanelMetrics::INSET),
            centre,
        ]);
    }
}

// MARK: - SourceLineIndex

/// Turns a UTF-16 source offset into a line number.
#[derive(Debug, Clone)]
pub struct SourceLineIndex {
    /// Offset of the first character of each line, line 1 first.
    starts: Vec<isize>,
}

impl SourceLineIndex {
    /// `SourceLineIndex(text:)`: `enumerateSubstrings(in:options:
    /// [.byLines, .substringNotRequired])`, through Foundation.
    pub fn new(text: &str) -> SourceLineIndex {
        let string = ns_string(text);
        let length = string.length();
        let starts = Rc::new(RefCell::new(vec![0isize]));
        let collected = starts.clone();
        let block = block2::RcBlock::new(
            move |_substring: *mut NSString, _range: NSRange, enclosing: NSRange, _stop: std::ptr::NonNull<objc2::runtime::Bool>| {
                let next = enclosing.location + enclosing.length;
                if next < length {
                    collected.borrow_mut().push(next as isize);
                }
            },
        );
        string.enumerateSubstringsInRange_options_usingBlock(
            NSRange::new(0, length),
            objc2_foundation::NSStringEnumerationOptions::ByLines
                | objc2_foundation::NSStringEnumerationOptions::SubstringNotRequired,
            &block,
        );
        drop(block);
        let starts = starts.borrow().clone();
        SourceLineIndex { starts }
    }

    /// 1-based line containing `offset`.
    pub fn line(&self, offset: isize) -> isize {
        if !(offset > 0) {
            return 1;
        }
        let mut low: isize = 0;
        let mut high = self.starts.len() as isize - 1;
        while low < high {
            let mid = (low + high + 1) / 2;
            if self.starts[mid as usize] <= offset {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        low + 1
    }

    /// "Line 42" or "Lines 42–44" — what a reader can act on.
    pub fn caption(&self, range: upleft_core::NSRange) -> String {
        let first = self.line(range.location);
        let last = self.line(range.location.max(range.location + range.length - 1));
        if first == last { format!("Line {first}") } else { format!("Lines {first}–{last}") }
    }
}

// MARK: - Names

/// `Command.panelTitle`: the command title without its ellipsis.
pub fn panel_title(command: Command) -> String {
    let title = command.title();
    match title.strip_suffix('…') {
        Some(stripped) => stripped.to_owned(),
        None => title.to_owned(),
    }
}

// MARK: - Small helpers

/// `Array.element(at:)`: bounds-checked subscript.
pub fn element_at<T>(slice: &[T], index: isize) -> Option<&T> {
    if index >= 0 { slice.get(index as usize) } else { None }
}

/// `NSView.enclosingInspectorHost`.
pub fn enclosing_inspector_host(view: &NSView) -> Option<Retained<super::inspector_host_view::InspectorHostView>> {
    let mut candidate = superview(&view);
    while let Some(view) = candidate {
        if let Some(host) = super::appkit_support::downcast::<super::inspector_host_view::InspectorHostView>(&view) {
            return Some(host);
        }
        candidate = superview(&view);
    }
    None
}

/// `NSView.installBackdrop(_:)`.
pub fn install_backdrop(view: &NSView, backdrop: &PanelBackdrop) {
    backdrop.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    backdrop.setFrame(view.bounds());
    view.addSubview(backdrop);
}

// Silence imports that only some configurations use.
#[allow(unused)]
fn _unused(_: CGPoint, _: CGSize, _: NSStackViewGravity, _: CATextLayerAlignmentMode, _: &dyn NSAppearanceCustomization) {}
