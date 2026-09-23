//! Port of `App/ToolbarGlassBand.swift`: the owned titlebar material. It
//! follows the content safe area instead of duplicating AppKit's
//! toolbar-height calculation.
//!
//! Objective-C name: `ToolbarGlassBand`.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAccessibility, NSAutoresizingMaskOptions, NSResponder, NSView};
use objc2_core_foundation::CGSize;
use objc2_foundation::NSPoint;
use upleft_render::appkit_compat::RECT_ZERO;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::panels::chrome_glass::{ChromeGlass, RoundedCorners, Tint};
use crate::panels::panel_chrome::PanelMetrics;

pub struct ToolbarGlassBandIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    glass: Retained<ChromeGlass>,
}

define_class!(
    /// The owned titlebar material.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // the overrides keep AppKit's signatures.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarGlassBand"]
    #[ivars = ToolbarGlassBandIvars]
    pub struct ToolbarGlassBand;

    unsafe impl NSObjectProtocol for ToolbarGlassBand {}

    impl ToolbarGlassBand {
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.ivars().glass.setFrame(self.bounds());
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl ToolbarGlassBand {
    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<ToolbarGlassBand> {
        let glass = ChromeGlass::new(
            style_sheet.clone(),
            PanelMetrics::BAND_CORNER_RADIUS,
            RoundedCorners::BottomOnly,
            Tint::Band,
            mtm,
        );
        let this = Self::alloc(mtm).set_ivars(ToolbarGlassBandIvars { style_sheet: RefCell::new(style_sheet), glass });
        let this: Retained<ToolbarGlassBand> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let glass = &this.ivars().glass;
        glass.set_passes_through_hits(true);
        glass.set_shadow_radius(12.0);
        glass.set_shadow_offset(CGSize::new(0.0, -3.0));
        glass.set_shadow_opacity(Some(0.10));
        glass.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(glass);
        this.setAccessibilityElement(false);
        this
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// `styleSheet { didSet { glass.styleSheet = styleSheet } }`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().glass.set_style_sheet(style_sheet);
    }

    pub fn glass_for_testing(&self) -> Retained<ChromeGlass> {
        self.ivars().glass.clone()
    }
}
