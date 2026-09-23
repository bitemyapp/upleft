//! Port of `Panels/ChromeGlass.swift`: one material implementation for
//! transient chrome. Content must be mounted in `content_view()`;
//! `NSGlassEffectView` only guarantees compositing order for the content view
//! it owns.

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, available, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAutoresizingMaskOptions, NSColor, NSColorSpace, NSGlassEffectView, NSGlassEffectViewStyle, NSResponder, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSWindowOrderingMode,
};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGMutablePath, CGPath};
use objc2_foundation::{NSPoint, NSRect};
use objc2_quartz_core::{CAGradientLayer, CALayer, CAShapeLayer, CATransaction, kCACornerCurveContinuous};
use upleft_render::motion::{self, SpringScalar, SpringSurfaceView};
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{set_mask, superview, IDENTITY, is_same_view, RectExt, cg, cg_array, null_actions, role, set_role, smax, smin};
use super::panel_chrome::{PanelBackdrop, PanelMetrics};

/// `ChromeGlass.RoundedCorners`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundedCorners {
    All,
    BottomOnly,
}

/// `ChromeGlass.Tint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Panel,
    Band,
    Control,
}

pub struct ChromeGlassIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    corner_radius: Cell<CGFloat>,
    rounded_corners: Cell<RoundedCorners>,
    tint: Cell<Tint>,
    shadow_radius: Cell<CGFloat>,
    shadow_offset: Cell<CGSize>,
    shadow_opacity: Cell<Option<f32>>,
    shows_focus: Cell<bool>,
    passes_through_hits: Cell<bool>,
    content_view: Retained<NSView>,
    fallback: Retained<PanelBackdrop>,
    rim_gradient: Retained<CAGradientLayer>,
    rim_mask: Retained<CAShapeLayer>,
    focus_layer: Retained<CAShapeLayer>,
    focus_opacity: RefCell<SpringScalar>,
    material: RefCell<Option<Retained<NSView>>>,
    uses_glass: Cell<bool>,
}

define_class!(
    /// `ChromeGlass`, a `Motion.SpringSurfaceView`.
    // SAFETY: `initWithFrame:` is forwarded to `SpringSurfaceView` in `new`
    // after the ivars are set.
    #[unsafe(super(SpringSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ChromeGlass"]
    #[ivars = ChromeGlassIvars]
    pub struct ChromeGlass;

    unsafe impl NSObjectProtocol for ChromeGlass {}

    impl ChromeGlass {
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            if let Some(material) = ivars.material.borrow().as_ref() {
                material.setFrame(self.bounds());
            }
            ivars.fallback.setFrame(self.bounds());
            ivars.content_view.setFrame(self.bounds());
            self.apply_style();
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            if self.ivars().passes_through_hits.get() {
                None
            } else {
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }

        #[unsafe(method(springTick:))]
        fn __spring_tick(&self, dt: CGFloat) -> bool {
            self.ivars().focus_opacity.borrow_mut().advance(dt)
        }

        #[unsafe(method(springApply))]
        fn __spring_apply(&self) {
            self.spring_apply();
        }

        #[unsafe(method(springsSettleImmediately))]
        fn __springs_settle_immediately(&self) {
            let target = if self.ivars().shows_focus.get() { 0.92 } else { 0.0 };
            self.ivars().focus_opacity.borrow_mut().snap(target);
            self.spring_apply();
        }
    }
);

impl ChromeGlass {
    /// `init(styleSheet:cornerRadius:roundedCorners:tint:)`; Swift's
    /// defaults are `.all` and `.panel`.
    pub fn new(
        style_sheet: Rc<StyleSheet>,
        corner_radius: CGFloat,
        rounded_corners: RoundedCorners,
        tint: Tint,
        mtm: MainThreadMarker,
    ) -> Retained<ChromeGlass> {
        let content_view = NSView::new(mtm);
        let fallback = PanelBackdrop::new(
            style_sheet.clone(),
            if tint == Tint::Band { NSVisualEffectMaterial::Titlebar } else { NSVisualEffectMaterial::HeaderView },
            NSVisualEffectBlendingMode::WithinWindow,
            mtm,
        );
        let this = Self::alloc(mtm).set_ivars(ChromeGlassIvars {
            style_sheet: RefCell::new(style_sheet),
            corner_radius: Cell::new(corner_radius),
            rounded_corners: Cell::new(rounded_corners),
            tint: Cell::new(tint),
            shadow_radius: Cell::new(18.0),
            shadow_offset: Cell::new(CGSize::new(0.0, -4.0)),
            shadow_opacity: Cell::new(None),
            shows_focus: Cell::new(false),
            passes_through_hits: Cell::new(false),
            content_view,
            fallback,
            rim_gradient: CAGradientLayer::new(),
            rim_mask: CAShapeLayer::new(),
            focus_layer: CAShapeLayer::new(),
            focus_opacity: RefCell::new(SpringScalar::new(0.0, 0.0, motion::SPRING_QUICK, 0.04)),
            material: RefCell::new(None),
            uses_glass: Cell::new(false),
        });
        let this: Retained<ChromeGlass> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.finish_init();
        this
    }

    pub fn supports_glass(style_sheet: &StyleSheet) -> bool {
        if !available!(macos = 26.0) {
            return false;
        }
        Self::supports_glass_with(true, style_sheet.reduce_transparency, style_sheet.increase_contrast)
    }

    /// `supportsGlass(osSupportsGlass:reduceTransparency:increaseContrast:)`.
    pub fn supports_glass_with(os_supports_glass: bool, reduce_transparency: bool, increase_contrast: bool) -> bool {
        os_supports_glass && !reduce_transparency && !increase_contrast
    }

    pub fn is_dark_background(color: &NSColor) -> bool {
        let Some(rgb) = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else { return false };
        let luminance = 0.2126 * rgb.redComponent() + 0.7152 * rgb.greenComponent() + 0.0722 * rgb.blueComponent();
        luminance < 0.5
    }

    pub fn glass_tint(style_sheet: &StyleSheet, tint: Tint) -> Retained<NSColor> {
        let dark = Self::is_dark_background(&style_sheet.background);
        let alpha: CGFloat = match tint {
            Tint::Panel => {
                if dark {
                    0.08
                } else {
                    0.025
                }
            }
            Tint::Band => {
                if dark {
                    0.16
                } else {
                    0.06
                }
            }
            Tint::Control => {
                if dark {
                    0.11
                } else {
                    0.035
                }
            }
        };
        style_sheet.background.colorWithAlphaComponent(alpha)
    }

    pub fn material_appearance(style_sheet: &StyleSheet) -> Option<Retained<NSAppearance>> {
        let name = unsafe {
            if Self::is_dark_background(&style_sheet.background) { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua }
        };
        NSAppearance::appearanceNamed(name)
    }

    pub fn opaque_fallback_color(style_sheet: &StyleSheet, tint: Tint) -> Retained<NSColor> {
        match tint {
            Tint::Band => style_sheet
                .surface
                .blendedColorWithFraction_ofColor(
                    if Self::is_dark_background(&style_sheet.background) { 0.30 } else { 0.55 },
                    &style_sheet.background,
                )
                .unwrap_or_else(|| style_sheet.surface.clone()),
            Tint::Panel | Tint::Control => style_sheet.surface.clone(),
        }
    }

    fn finish_init(&self) {
        let ivars = self.ivars();
        self.setWantsLayer(true);
        if let Some(layer) = self.layer() {
            layer.setMasksToBounds(false);
            layer.setShadowColor(Some(&cg(&NSColor::blackColor())));
        }

        let style_sheet = self.style_sheet();
        ivars.fallback.set_blends_within_window(true);
        ivars.fallback.set_uses_surface_fill(style_sheet.reduce_transparency || style_sheet.increase_contrast);
        ivars.fallback.set_opaque_surface_color(Some(Self::opaque_fallback_color(&style_sheet, ivars.tint.get())));

        ivars.content_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        set_role(&*ivars.content_view, role::group());

        ivars.rim_mask.setFillColor(Some(&cg(&NSColor::clearColor())));
        ivars.rim_mask.setStrokeColor(Some(&cg(&NSColor::whiteColor())));
        null_actions(&ivars.rim_mask, &["path", "frame", "lineWidth"]);
        set_mask(&ivars.rim_gradient, Some(&ivars.rim_mask));
        null_actions(&ivars.rim_gradient, &["frame", "colors", "isHidden"]);
        ivars.rim_gradient.setZPosition(100.0);
        if let Some(layer) = self.layer() {
            layer.addSublayer(&ivars.rim_gradient);
        }

        ivars.focus_layer.setFillColor(Some(&cg(&NSColor::clearColor())));
        null_actions(&ivars.focus_layer, &["path", "frame", "strokeColor", "opacity"]);
        ivars.focus_layer.setZPosition(101.0);
        if let Some(layer) = self.layer() {
            layer.addSublayer(&ivars.focus_layer);
        }

        set_role(self, role::group());
        self.update_material();
    }

    pub fn content_view(&self) -> Retained<NSView> {
        self.ivars().content_view.clone()
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.update_material();
    }

    pub fn corner_radius(&self) -> CGFloat {
        self.ivars().corner_radius.get()
    }

    pub fn set_corner_radius(&self, value: CGFloat) {
        self.ivars().corner_radius.set(value);
        self.apply_style();
    }

    pub fn rounded_corners(&self) -> RoundedCorners {
        self.ivars().rounded_corners.get()
    }

    pub fn set_rounded_corners(&self, value: RoundedCorners) {
        self.ivars().rounded_corners.set(value);
        self.apply_style();
    }

    pub fn tint(&self) -> Tint {
        self.ivars().tint.get()
    }

    pub fn set_tint(&self, value: Tint) {
        self.ivars().tint.set(value);
        self.update_material();
    }

    pub fn set_shadow_radius(&self, value: CGFloat) {
        self.ivars().shadow_radius.set(value);
        self.apply_style();
    }

    pub fn set_shadow_offset(&self, value: CGSize) {
        self.ivars().shadow_offset.set(value);
        self.apply_style();
    }

    pub fn set_shadow_opacity(&self, value: Option<f32>) {
        self.ivars().shadow_opacity.set(value);
        self.apply_style();
    }

    pub fn shows_focus(&self) -> bool {
        self.ivars().shows_focus.get()
    }

    pub fn set_shows_focus(&self, value: bool) {
        let old_value = self.ivars().shows_focus.get();
        self.ivars().shows_focus.set(value);
        if value == old_value {
            return;
        }
        self.update_focus(self.window().is_some());
    }

    pub fn passes_through_hits(&self) -> bool {
        self.ivars().passes_through_hits.get()
    }

    pub fn set_passes_through_hits(&self, value: bool) {
        self.ivars().passes_through_hits.set(value);
    }

    pub fn uses_glass(&self) -> bool {
        self.ivars().uses_glass.get()
    }

    fn glass_effect(&self) -> Option<Retained<NSGlassEffectView>> {
        let material = self.ivars().material.borrow().clone()?;
        super::appkit_support::downcast::<NSGlassEffectView>(&material)
    }

    fn update_material(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        let appearance = Self::material_appearance(&style_sheet);
        self.setAppearance(appearance.as_deref());
        let appearance = self.appearance();
        ivars.content_view.setAppearance(appearance.as_deref());
        ivars.fallback.setAppearance(appearance.as_deref());
        let wants_glass = Self::supports_glass(&style_sheet);
        if wants_glass {
            if !ivars.uses_glass.get() && available!(macos = 26.0) {
                self.mount_glass();
            }
            if available!(macos = 26.0) {
                let tint = ivars.tint.get();
                if let Some(glass) = self.glass_effect() {
                    glass.setStyle(if tint == Tint::Panel {
                        NSGlassEffectViewStyle::Clear
                    } else {
                        NSGlassEffectViewStyle::Regular
                    });
                }
                if let Some(glass) = self.glass_effect() {
                    glass.setAppearance(Self::material_appearance(&style_sheet).as_deref());
                }
                if let Some(glass) = self.glass_effect() {
                    glass.setTintColor(Some(&Self::glass_tint(&style_sheet, tint)));
                }
            }
        } else if ivars.uses_glass.get() || !is_same_view(superview(&ivars.content_view), self) {
            self.mount_fallback();
        }
        ivars.fallback.set_style_sheet(style_sheet.clone());
        ivars.fallback.set_uses_surface_fill(style_sheet.reduce_transparency || style_sheet.increase_contrast);
        ivars.fallback.set_opaque_surface_color(Some(Self::opaque_fallback_color(&style_sheet, ivars.tint.get())));
        self.apply_style();
    }

    fn mount_glass(&self) {
        let ivars = self.ivars();
        let mtm = self.mtm();
        ivars.content_view.removeFromSuperview();
        ivars.fallback.removeFromSuperview();
        if let Some(material) = ivars.material.borrow().as_ref() {
            material.removeFromSuperview();
        }

        let style_sheet = self.style_sheet();
        let tint = ivars.tint.get();
        let glass = NSGlassEffectView::new(mtm);
        glass.setStyle(if tint == Tint::Panel { NSGlassEffectViewStyle::Clear } else { NSGlassEffectViewStyle::Regular });
        glass.setCornerRadius(if ivars.rounded_corners.get() == RoundedCorners::All {
            ivars.corner_radius.get()
        } else {
            0.0
        });
        glass.setAppearance(Self::material_appearance(&style_sheet).as_deref());
        glass.setTintColor(Some(&Self::glass_tint(&style_sheet, tint)));
        glass.setWantsLayer(true);
        if let Some(layer) = glass.layer() {
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        }
        glass.setClipsToBounds(true);
        glass.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        glass.setFrame(self.bounds());
        glass.setContentView(Some(&ivars.content_view));
        self.addSubview(&glass);
        *ivars.material.borrow_mut() = Some(Retained::into_super(glass));
        ivars.uses_glass.set(true);
        PanelBackdrop::resolve_detached_glass(&ivars.content_view);
    }

    fn mount_fallback(&self) {
        let ivars = self.ivars();
        ivars.content_view.removeFromSuperview();
        if let Some(material) = ivars.material.borrow().as_ref() {
            material.removeFromSuperview();
        }
        *ivars.material.borrow_mut() = None;
        ivars.uses_glass.set(false);
        ivars.fallback.setFrame(self.bounds());
        ivars
            .fallback
            .setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        if !is_same_view(superview(&ivars.fallback), self) {
            self.addSubview(&ivars.fallback);
        }
        ivars.content_view.setFrame(self.bounds());
        if !is_same_view(superview(&ivars.content_view), self) {
            self.addSubview_positioned_relativeTo(&ivars.content_view, NSWindowOrderingMode::Above, Some(&ivars.fallback));
        }
        PanelBackdrop::resolve_detached_glass(&ivars.content_view);
    }

    pub fn refresh_glass_after_window_attach(&self) {
        if !(self.ivars().uses_glass.get() && available!(macos = 26.0)) {
            return;
        }
        self.mount_glass();
        self.apply_style();
    }

    fn apply_style(&self) {
        let Some(layer) = self.layer() else { return };
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        let bounds = self.bounds();
        let path = self.chrome_path(bounds, 0.0);
        let inset_path = self.chrome_path(bounds.inset_by(0.75, 0.75), 0.75);
        let dark = Self::is_dark_background(&style_sheet.background);

        layer.setShadowRadius(ivars.shadow_radius.get());
        layer.setShadowOffset(ivars.shadow_offset.get());
        layer.setShadowOpacity(ivars.shadow_opacity.get().unwrap_or(if style_sheet.increase_contrast {
            0.30
        } else if dark {
            0.28
        } else {
            0.16
        }));
        layer.setShadowPath(Some(&path));

        if let Some(material) = ivars.material.borrow().as_ref()
            && let Some(material_layer) = material.layer()
        {
            set_mask(&material_layer, Some(&self.mask_layer(&path)));
        }
        if let Some(fallback_layer) = ivars.fallback.layer() {
            set_mask(&fallback_layer, Some(&self.mask_layer(&path)));
        }
        if available!(macos = 26.0)
            && let Some(glass) = self.glass_effect()
        {
            glass.setCornerRadius(if ivars.rounded_corners.get() == RoundedCorners::All {
                ivars.corner_radius.get()
            } else {
                0.0
            });
        }

        ivars.rim_gradient.setFrame(bounds);
        ivars.rim_mask.setFrame(bounds);
        ivars.rim_mask.setPath(Some(&inset_path));
        ivars.rim_mask.setLineWidth(if style_sheet.increase_contrast { 1.5 } else { 1.0 });
        let rim_alpha: CGFloat = if style_sheet.increase_contrast {
            0.95
        } else if dark {
            0.72
        } else {
            0.86
        };
        let white = NSColor::whiteColor();
        let colors = cg_array(&[
            cg(&white.colorWithAlphaComponent(rim_alpha)),
            cg(&white.colorWithAlphaComponent(rim_alpha * 0.42)),
            cg(&white.colorWithAlphaComponent(if dark { 0.10 } else { 0.16 })),
            cg(&white.colorWithAlphaComponent(0.025)),
        ]);
        unsafe { ivars.rim_gradient.setColors(Some(&colors)) };
        ivars.rim_gradient.setStartPoint(CGPoint::new(0.08, 0.98));
        ivars.rim_gradient.setEndPoint(CGPoint::new(0.92, 0.02));
        ivars.rim_gradient.setHidden(ivars.uses_glass.get() && !style_sheet.increase_contrast);

        ivars.focus_layer.setFrame(bounds);
        ivars.focus_layer.setPath(Some(&self.chrome_path(bounds.inset_by(1.5, 1.5), 1.5)));
        ivars.focus_layer.setStrokeColor(Some(&cg(&style_sheet.accent)));
        ivars.focus_layer.setLineWidth(1.5);
        ivars.focus_layer.setOpacity(ivars.focus_opacity.borrow().value() as f32);
        CATransaction::commit();
    }

    fn update_focus(&self, animated: bool) {
        let target: CGFloat = if self.ivars().shows_focus.get() { 0.92 } else { 0.0 };
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        if !(animated && self.window().is_some() && !reduce_motion) {
            self.park_springs();
            self.ivars().focus_opacity.borrow_mut().snap(target);
            self.spring_apply();
            return;
        }
        self.ivars().focus_opacity.borrow_mut().target(target);
        if !self.arm_springs() {
            self.ivars().focus_opacity.borrow_mut().snap(target);
            self.spring_apply();
        }
    }

    fn spring_apply(&self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.ivars().focus_layer.setOpacity(self.ivars().focus_opacity.borrow().value() as f32);
        CATransaction::commit();
    }

    fn mask_layer(&self, path: &CGPath) -> Retained<CAShapeLayer> {
        let mask = CAShapeLayer::new();
        mask.setFrame(self.bounds());
        mask.setPath(Some(path));
        mask.setFillColor(Some(&cg(&NSColor::blackColor())));
        null_actions(&mask, &["path", "frame"]);
        mask
    }

    fn chrome_path(&self, rect: CGRect, inset: CGFloat) -> CFRetained<CGPath> {
        let radius = smax(0.0, self.ivars().corner_radius.get() - inset);
        match self.ivars().rounded_corners.get() {
            RoundedCorners::All => PanelMetrics::continuous_rounded_path(rect, radius),
            RoundedCorners::BottomOnly => Self::bottom_rounded_path(rect, radius),
        }
    }

    fn bottom_rounded_path(rect: CGRect, radius: CGFloat) -> CFRetained<CGPath> {
        let r = smin(smax(0.0, radius), smin(rect.width(), rect.height()) / 2.0);
        let control = r * 0.447_715;
        let path = CGMutablePath::new();
        let p = Some(&*path);
        // SAFETY: `IDENTITY` outlives each call.
        unsafe {
            CGMutablePath::move_to_point(p, &IDENTITY, rect.min_x(), rect.max_y());
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.max_x(), rect.max_y());
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.max_x(), rect.min_y() + r);
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.max_x(),
                rect.min_y() + control,
                rect.max_x() - control,
                rect.min_y(),
                rect.max_x() - r,
                rect.min_y(),
            );
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.min_x() + r, rect.min_y());
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.min_x() + control,
                rect.min_y(),
                rect.min_x(),
                rect.min_y() + control,
                rect.min_x(),
                rect.min_y() + r,
            );
        }
        CGMutablePath::close_subpath(p);
        super::appkit_support::immutable(path)
    }

    pub fn renders_opaque_fallback_for_testing(&self) -> bool {
        !self.ivars().uses_glass.get() && is_same_view(superview(&self.ivars().fallback), self)
    }
}

#[allow(unused)]
fn _unused(_: &CALayer) {}
