//! The Swift `CGContext` / `CGMutablePath` overlay methods the renderers call,
//! each one the single CoreGraphics function the overlay forwards to (with
//! the identity transform pointer the path overlay passes by default).

use objc2_core_foundation::{CFRetained, CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGContext, CGLineCap, CGLineJoin, CGMutablePath, CGPath, CGPathDrawingMode};

use crate::cross_platform::IDENTITY;

/// A borrowed `CGContext` with the Swift overlay's method names.
#[derive(Clone, Copy)]
pub struct Ctx<'a>(pub &'a CGContext);

impl<'a> Ctx<'a> {
    #[inline]
    fn c(self) -> Option<&'a CGContext> {
        Some(self.0)
    }
    pub fn save_g_state(self) {
        CGContext::save_g_state(self.c());
    }
    pub fn restore_g_state(self) {
        CGContext::restore_g_state(self.c());
    }
    pub fn translate_by(self, x: CGFloat, y: CGFloat) {
        CGContext::translate_ctm(self.c(), x, y);
    }
    pub fn scale_by(self, x: CGFloat, y: CGFloat) {
        CGContext::scale_ctm(self.c(), x, y);
    }
    pub fn rotate(self, angle: CGFloat) {
        CGContext::rotate_ctm(self.c(), angle);
    }
    pub fn set_fill_color(self, color: &CGColor) {
        CGContext::set_fill_color_with_color(self.c(), Some(color));
    }
    pub fn set_stroke_color(self, color: &CGColor) {
        CGContext::set_stroke_color_with_color(self.c(), Some(color));
    }
    pub fn set_line_width(self, width: CGFloat) {
        CGContext::set_line_width(self.c(), width);
    }
    pub fn set_line_cap(self, cap: CGLineCap) {
        CGContext::set_line_cap(self.c(), cap);
    }
    pub fn set_line_join(self, join: CGLineJoin) {
        CGContext::set_line_join(self.c(), join);
    }
    pub fn set_line_dash(self, phase: CGFloat, lengths: &[CGFloat]) {
        unsafe { CGContext::set_line_dash(self.c(), phase, lengths.as_ptr(), lengths.len()) };
    }
    pub fn set_alpha(self, alpha: CGFloat) {
        CGContext::set_alpha(self.c(), alpha);
    }
    pub fn fill(self, rect: CGRect) {
        CGContext::fill_rect(self.c(), rect);
    }
    pub fn stroke(self, rect: CGRect) {
        CGContext::stroke_rect(self.c(), rect);
    }
    pub fn fill_ellipse(self, rect: CGRect) {
        CGContext::fill_ellipse_in_rect(self.c(), rect);
    }
    pub fn stroke_ellipse(self, rect: CGRect) {
        CGContext::stroke_ellipse_in_rect(self.c(), rect);
    }
    pub fn add_ellipse(self, rect: CGRect) {
        CGContext::add_ellipse_in_rect(self.c(), rect);
    }
    pub fn move_to(self, p: CGPoint) {
        CGContext::move_to_point(self.c(), p.x, p.y);
    }
    pub fn add_line(self, p: CGPoint) {
        CGContext::add_line_to_point(self.c(), p.x, p.y);
    }
    pub fn add_curve(self, to: CGPoint, control1: CGPoint, control2: CGPoint) {
        CGContext::add_curve_to_point(self.c(), control1.x, control1.y, control2.x, control2.y, to.x, to.y);
    }
    pub fn add_path(self, path: &CGPath) {
        CGContext::add_path(self.c(), Some(path));
    }
    pub fn stroke_path(self) {
        CGContext::stroke_path(self.c());
    }
    pub fn fill_path(self) {
        CGContext::fill_path(self.c());
    }
    pub fn draw_path(self, mode: CGPathDrawingMode) {
        CGContext::draw_path(self.c(), mode);
    }
    pub fn set_text_position(self, p: CGPoint) {
        CGContext::set_text_position(self.c(), p.x, p.y);
    }
}

#[inline]
pub fn pt(x: CGFloat, y: CGFloat) -> CGPoint {
    CGPoint { x, y }
}

#[inline]
pub fn rect(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat) -> CGRect {
    CGRect { origin: CGPoint { x, y }, size: CGSize { width, height } }
}

/// `CGMutablePath` with the overlay's defaulted `transform: .identity`.
pub struct Path(pub CFRetained<CGMutablePath>);

impl Path {
    pub fn new() -> Path {
        Path(CGMutablePath::new())
    }
    pub fn move_to(&self, p: CGPoint) {
        unsafe { CGMutablePath::move_to_point(Some(&self.0), &IDENTITY, p.x, p.y) };
    }
    pub fn add_line(&self, p: CGPoint) {
        unsafe { CGMutablePath::add_line_to_point(Some(&self.0), &IDENTITY, p.x, p.y) };
    }
    pub fn add_quad_curve(&self, to: CGPoint, control: CGPoint) {
        unsafe { CGMutablePath::add_quad_curve_to_point(Some(&self.0), &IDENTITY, control.x, control.y, to.x, to.y) };
    }
    pub fn add_rounded_rect(&self, rect: CGRect, corner_width: CGFloat, corner_height: CGFloat) {
        unsafe { CGMutablePath::add_rounded_rect(Some(&self.0), &IDENTITY, rect, corner_width, corner_height) };
    }
    pub fn add_ellipse(&self, rect: CGRect) {
        unsafe { CGMutablePath::add_ellipse_in_rect(Some(&self.0), &IDENTITY, rect) };
    }
    pub fn close_subpath(&self) {
        CGMutablePath::close_subpath(Some(&self.0));
    }
    pub fn as_path(&self) -> &CGPath {
        &self.0
    }
}

impl Default for Path {
    fn default() -> Self {
        Path::new()
    }
}

/// `CGPath(rect:transform: nil)`.
pub fn path_rect(rect: CGRect) -> CFRetained<CGPath> {
    unsafe { CGPath::with_rect(rect, std::ptr::null()) }
}

/// `CGPath(ellipseIn:transform: nil)`.
pub fn path_ellipse(rect: CGRect) -> CFRetained<CGPath> {
    unsafe { CGPath::with_ellipse_in_rect(rect, std::ptr::null()) }
}

/// `CGPath(roundedRect:cornerWidth:cornerHeight:transform: nil)`.
pub fn path_rounded_rect(rect: CGRect, corner_width: CGFloat, corner_height: CGFloat) -> CFRetained<CGPath> {
    unsafe { CGPath::with_rounded_rect(rect, corner_width, corner_height, std::ptr::null()) }
}

/// `path.copy(using: &transform)`.
pub fn path_copy_transformed(path: &CGPath, transform: &CGAffineTransform) -> Option<CFRetained<CGPath>> {
    unsafe { CGPath::new_copy_by_transforming_path(Some(path), transform) }
}

/// `CGRect.insetBy(dx:dy:)`.
pub fn inset(r: CGRect, dx: CGFloat, dy: CGFloat) -> CGRect {
    unsafe extern "C-unwind" {
        fn CGRectInset(rect: CGRect, dx: CGFloat, dy: CGFloat) -> CGRect;
    }
    unsafe { CGRectInset(r, dx, dy) }
}

pub fn min_x(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetMinX(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetMinX(r) }
}
pub fn min_y(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetMinY(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetMinY(r) }
}
pub fn max_x(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetMaxX(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetMaxX(r) }
}
pub fn max_y(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetMaxY(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetMaxY(r) }
}
pub fn mid_x(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetMidX(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetMidX(r) }
}
pub fn mid_y(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetMidY(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetMidY(r) }
}
pub fn width(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetWidth(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetWidth(r) }
}
pub fn height(r: CGRect) -> CGFloat {
    unsafe extern "C-unwind" {
        fn CGRectGetHeight(rect: CGRect) -> CGFloat;
    }
    unsafe { CGRectGetHeight(r) }
}
