//! Not a Swift test: pins the `DownrightFragment` subclassing seam the
//! object-fragment port builds on. A behaviour registered under a Swift
//! class name must produce an instance of an Objective-C class with exactly
//! that name, and the base must apply its hooks the way Swift's
//! `DownrightFragment` does.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::NSTextParagraph;
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSAttributedString, NSString};
use upleft_core::{BlockIdentity, NSRange};
use upleft_render::fragments::fragment_base::{
    DownrightFragment, FragmentBehavior, FragmentContext, downright_fragment_class,
};
use upleft_render::render_contracts::{FragmentKind, FragmentPayload};

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("fragment_seam_class_name_and_hooks", class_name_and_hooks),
    ("fragment_seam_padding_extends_frame", padding_extends_frame),
];

struct FixedHeight {
    height: CGFloat,
    asked: Cell<usize>,
}

impl FragmentBehavior for FixedHeight {
    fn override_height(&self, _fragment: &DownrightFragment) -> Option<CGFloat> {
        self.asked.set(self.asked.get() + 1);
        Some(self.height)
    }
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Padded;

impl FragmentBehavior for Padded {
    fn vertical_padding(&self, _fragment: &DownrightFragment) -> (CGFloat, CGFloat) {
        (6.0, 10.0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn element() -> objc2::rc::Retained<NSTextParagraph> {
    let string = NSAttributedString::from_nsstring(&NSString::from_str("Row text"));
    NSTextParagraph::initWithAttributedString(NSTextParagraph::alloc(), Some(&string))
}

fn payload() -> objc2::rc::Retained<FragmentPayload> {
    FragmentPayload::new(FragmentKind::ThematicBreak, NSRange::new(0, 8), BlockIdentity::new(0, 0), "")
}

fn class_name_and_hooks(_mtm: MainThreadMarker) {
    let context = FragmentContext::new(fallback_sheet());
    let element = element();
    let payload = payload();
    let fragment = DownrightFragment::new(
        c"SeamProbeFragment",
        &element,
        None,
        &payload,
        &context,
        Box::new(FixedHeight { height: 42.0, asked: Cell::new(0) }),
    );
    let object: &AnyObject = &fragment;
    expect!(object.class().name().to_str().unwrap() == "SeamProbeFragment");
    expect!(std::ptr::eq(object.class(), downright_fragment_class(c"SeamProbeFragment")));
    // The overridden getter is what AppKit (and the layout dump) sees.
    expect!(fragment.layoutFragmentFrame().size.height == 42.0);
    let behavior = fragment.behavior().as_any().downcast_ref::<FixedHeight>().expect("behaviour");
    expect!(behavior.asked.get() >= 1);
    expect!(fragment.suppresses_text());
    expect!(fragment.payload().kind() == FragmentKind::ThematicBreak);
    // A second instance reuses the registered class.
    let again = DownrightFragment::new(
        c"SeamProbeFragment",
        &element,
        None,
        &payload,
        &context,
        Box::new(Padded),
    );
    let again_object: &AnyObject = &again;
    expect!(std::ptr::eq(again_object.class(), object.class()));
    drop(Rc::clone(&context));
}

fn padding_extends_frame(_mtm: MainThreadMarker) {
    let context = FragmentContext::new(fallback_sheet());
    let element = element();
    let payload = payload();
    let fragment = DownrightFragment::new(c"SeamPaddedFragment", &element, None, &payload, &context, Box::new(Padded));
    let natural = fragment.super_layout_fragment_frame().size.height;
    expect!(fragment.layoutFragmentFrame().size.height == natural + 16.0);
    // `renderingSurfaceBounds` spans the column plus the reveal slack.
    let surface = fragment.renderingSurfaceBounds();
    expect!(surface.origin.x <= -upleft_render::engine::render_metrics::REVEAL_SLACK);
}
