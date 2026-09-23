//! Port of `Fragments/ThematicBreakFragment.swift`: a horizontal rule as a
//! hairline with generous space (§11.3), not a thick divider.

use std::any::Any;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::{NSTextElement, NSTextLayoutFragment, NSTextRange};
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_core_graphics::CGContext;

use crate::appkit_compat::{RectExt, rect};
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{DownrightFragment, FragmentBehavior, FragmentContext};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::smax;

/// `ThematicBreakFragment`'s hooks.
pub struct ThematicBreakFragment;

/// `ThematicBreakFragment(textElement:range:payload:context:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Retained<NSTextLayoutFragment> {
    Retained::into_super(DownrightFragment::new(
        c"ThematicBreakFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(ThematicBreakFragment),
    ))
}

impl FragmentBehavior for ThematicBreakFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }

    fn override_height(&self, fragment: &DownrightFragment) -> Option<CGFloat> {
        if !fragment.is_first_paragraph_of_block() {
            return Some(0.0);
        }
        let style = fragment.style_sheet()?;
        Some(render_metrics::snap_up(render_metrics::THEMATIC_BREAK_SPACE * 2.0, smax(1.0, style.baseline_grid)))
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        if !fragment.is_first_paragraph_of_block() {
            return;
        }
        let Some(style) = fragment.style_sheet() else { return };
        let height = fragment.layoutFragmentFrame().height();
        // Derived from the type ramp, not fixed.
        let body_size = style.body_font().pointSize();
        let diameter = smax(2.0, (body_size * 0.18).round());
        let gap = render_metrics::snap_up(body_size * 1.9, smax(1.0, style.baseline_grid));
        // Centred on the reading column, not the full one.
        let centre = point.x + fragment.prose_content_width() / 2.0;
        let y = point.y + height / 2.0 - diameter / 2.0;
        let context = Some(cg);
        CGContext::set_fill_color_with_color(context, Some(&style.text_faint.CGColor()));
        for offset in [-gap, 0.0, gap] {
            CGContext::fill_ellipse_in_rect(context, rect(centre + offset - diameter / 2.0, y, diameter, diameter));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
