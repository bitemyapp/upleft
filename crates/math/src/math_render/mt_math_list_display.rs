//! `MTMathListDisplay.swift`: the typeset display tree and its drawing.
//!
//! `MTDisplay` and its subclasses become one struct with the shared stored
//! properties and a [`DisplayKind`] for the subclass. The Swift subclasses
//! override the `ascent`/`descent`/`width` getters and the `position` and
//! `textColor` setters; [`MTDisplay::ascent`], [`MTDisplay::set_position`]
//! and friends dispatch the same way.
//!
//! Drawing makes the same Core Graphics, Core Text and AppKit calls in the
//! same order. The stroked rules and the radical's fill go through
//! `NSColor.setStroke`/`setFill` and `NSBezierPath`, which act on the
//! *current* `NSGraphicsContext` — as in SwiftMath, `draw` must run inside
//! one whose `CGContext` is `context` (an `NSImage` drawing handler).

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBezierPath, NSColor, NSLineCapStyle};
use objc2_core_foundation::{
    CFAttributedString, CFRetained, CFString, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGBlendMode, CGContext, CGGlyph, CGRectGetMaxY, CGRectGetMinY};
use objc2_core_text::{CTLine, CTLineBoundsOptions, kCTForegroundColorAttributeName};
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSNotFound, NSRange, NSString,
};
use std::sync::Arc;

use super::mt_font::MTFont;
use super::mt_font_math_table::MTFontMathTable;
use super::mt_math_list::MTMathAtomRef;
use crate::swift;

/// `NSNotFound` as an `NSRange` location.
pub const NS_NOT_FOUND: usize = NSNotFound as usize;

/// The type of position for a line, i.e. subscript/superscript or regular.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(isize)]
pub enum LinePosition {
    /// Regular
    #[default]
    Regular = 0,
    /// Positioned at a subscript
    Subscript,
    /// Positioned at a superscript
    Superscript,
}

/// A rendering of a single CTLine.
#[derive(Debug)]
pub struct MTCTLineDisplay {
    /// The CTLine being displayed
    pub line: CFRetained<CTLine>,
    /// The attributed string used to generate the CTLine.
    pub attributed_string: Retained<NSAttributedString>,
    /// The MTMathAtoms that this CTLine displays.
    pub atoms: Vec<MTMathAtomRef>,
}

/// An MTLine: a rendered form of MTMathList in one line.
#[derive(Debug, Default)]
pub struct MTMathListDisplay {
    /// Where the line is positioned
    pub type_: LinePosition,
    /// MTDisplays positioned relative to the position of this display.
    pub sub_displays: Vec<MTDisplay>,
    /// For a subscript or superscript, the location in the parent list; NSNotFound otherwise.
    pub index: isize,
}

#[derive(Debug, Default)]
pub struct MTFractionDisplay {
    pub numerator: Option<Box<MTDisplay>>,
    pub denominator: Option<Box<MTDisplay>>,
    numerator_up: CGFloat,
    denominator_down: CGFloat,
    pub line_position: CGFloat,
    pub line_thickness: CGFloat,
}

impl MTFractionDisplay {
    pub fn numerator_up(&self) -> CGFloat {
        self.numerator_up
    }

    pub fn denominator_down(&self) -> CGFloat {
        self.denominator_down
    }
}

#[derive(Debug)]
pub struct MTRadicalDisplay {
    pub radicand: Option<Box<MTDisplay>>,
    pub degree: Option<Box<MTDisplay>>,
    radical_glyph: Option<Box<MTDisplay>>,
    radical_shift: CGFloat,
    pub top_kern: CGFloat,
    pub line_thickness: CGFloat,
}

impl MTRadicalDisplay {
    /// `_radicalGlyph`.
    pub fn radical_glyph(&self) -> Option<&MTDisplay> {
        self.radical_glyph.as_deref()
    }

    /// `_radicalShift`.
    pub fn radical_shift(&self) -> CGFloat {
        self.radical_shift
    }
}

#[derive(Debug)]
pub struct MTGlyphDisplay {
    pub shift_down: CGFloat,
    pub glyph: CGGlyph,
    pub font: Option<Arc<MTFont>>,
}

#[derive(Debug)]
pub struct MTGlyphConstructionDisplay {
    pub shift_down: CGFloat,
    pub glyphs: Vec<CGGlyph>,
    pub positions: Vec<CGPoint>,
    pub font: Option<Arc<MTFont>>,
    pub num_glyphs: usize,
}

#[derive(Debug)]
pub struct MTLargeOpLimitsDisplay {
    pub upper_limit: Option<Box<MTDisplay>>,
    pub lower_limit: Option<Box<MTDisplay>>,
    pub limit_shift: CGFloat,
    upper_limit_gap: CGFloat,
    lower_limit_gap: CGFloat,
    pub extra_padding: CGFloat,
    pub nucleus: Option<Box<MTDisplay>>,
}

impl MTLargeOpLimitsDisplay {
    pub fn upper_limit_gap(&self) -> CGFloat {
        self.upper_limit_gap
    }

    pub fn lower_limit_gap(&self) -> CGFloat {
        self.lower_limit_gap
    }
}

#[derive(Debug)]
pub struct MTLineDisplay {
    pub inner: Option<Box<MTDisplay>>,
    pub line_shift_up: CGFloat,
    pub line_thickness: CGFloat,
}

#[derive(Debug)]
pub struct MTAccentDisplay {
    pub accentee: Option<Box<MTDisplay>>,
    pub accent: Option<Box<MTDisplay>>,
}

/// The Swift subclass of a display.
#[derive(Debug)]
pub enum DisplayKind {
    CTLine(MTCTLineDisplay),
    MathList(MTMathListDisplay),
    Fraction(MTFractionDisplay),
    Radical(MTRadicalDisplay),
    Glyph(MTGlyphDisplay),
    GlyphConstruction(MTGlyphConstructionDisplay),
    LargeOpLimits(MTLargeOpLimitsDisplay),
    Line(MTLineDisplay),
    Accent(MTAccentDisplay),
}

impl DisplayKind {
    pub fn class_name(&self) -> &'static str {
        match self {
            DisplayKind::CTLine(_) => "MTCTLineDisplay",
            DisplayKind::MathList(_) => "MTMathListDisplay",
            DisplayKind::Fraction(_) => "MTFractionDisplay",
            DisplayKind::Radical(_) => "MTRadicalDisplay",
            DisplayKind::Glyph(_) => "MTGlyphDisplay",
            DisplayKind::GlyphConstruction(_) => "MTGlyphConstructionDisplay",
            DisplayKind::LargeOpLimits(_) => "MTLargeOpLimitsDisplay",
            DisplayKind::Line(_) => "MTLineDisplay",
            DisplayKind::Accent(_) => "MTAccentDisplay",
        }
    }
}

/// The base class for rendering a math equation.
#[derive(Debug)]
pub struct MTDisplay {
    ascent: CGFloat,
    descent: CGFloat,
    width: CGFloat,
    position: CGPoint,
    /// The range of characters supported by this item
    pub range: NSRange,
    /// Whether the display has a subscript/superscript following it.
    pub has_script: bool,
    text_color: Option<Retained<NSColor>>,
    /// The local color, if the color was mutated local with the color command
    pub local_text_color: Option<Retained<NSColor>>,
    /// The background color for this display
    pub local_background_color: Option<Retained<NSColor>>,
    pub kind: DisplayKind,
}

fn zero() -> CGPoint {
    CGPoint::new(0.0, 0.0)
}

impl MTDisplay {
    fn base(kind: DisplayKind) -> MTDisplay {
        MTDisplay {
            ascent: 0.0,
            descent: 0.0,
            width: 0.0,
            position: zero(),
            range: NSRange::new(0, 0),
            has_script: false,
            text_color: None,
            local_text_color: None,
            local_background_color: None,
            kind,
        }
    }

    // MARK: - Constructors

    /// `MTCTLineDisplay(withString:position:range:font:atoms:)`.
    pub fn ct_line(
        attr_string: Retained<NSAttributedString>,
        position: CGPoint,
        range: NSRange,
        _font: Option<&Arc<MTFont>>,
        atoms: Vec<MTMathAtomRef>,
    ) -> MTDisplay {
        let line = create_line(&attr_string);
        let mut display = MTDisplay::base(DisplayKind::CTLine(MTCTLineDisplay {
            line,
            attributed_string: attr_string,
            atoms,
        }));
        display.position = position;
        display.range = range;
        let DisplayKind::CTLine(ct) = &display.kind else {
            unreachable!()
        };
        // We can't use typographic bounds here as the ascent and descent returned are for the font and not for the line.
        display.width = unsafe {
            ct.line.typographic_bounds(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let bounds = unsafe {
            ct.line
                .bounds_with_options(CTLineBoundsOptions::UseGlyphPathBounds)
        };
        display.ascent = swift::max(0.0, CGRectGetMaxY(bounds) - 0.0);
        display.descent = swift::max(0.0, 0.0 - CGRectGetMinY(bounds));
        display
    }

    /// `MTMathListDisplay(withDisplays:range:)`.
    pub fn math_list(displays: Vec<MTDisplay>, range: NSRange) -> MTDisplay {
        let mut display = MTDisplay::base(DisplayKind::MathList(MTMathListDisplay {
            sub_displays: displays,
            type_: LinePosition::Regular,
            index: NSNotFound,
        }));
        display.position = zero();
        display.range = range;
        display.recompute_dimensions();
        display
    }

    /// `MTFractionDisplay(withNumerator:denominator:position:range:)`.
    pub fn fraction(
        numerator: MTDisplay,
        denominator: MTDisplay,
        position: CGPoint,
        range: NSRange,
    ) -> MTDisplay {
        let mut display = MTDisplay::base(DisplayKind::Fraction(MTFractionDisplay {
            numerator: Some(Box::new(numerator)),
            denominator: Some(Box::new(denominator)),
            ..MTFractionDisplay::default()
        }));
        display.set_position(position);
        display.range = range;
        display
    }

    /// `MTRadicalDisplay(withRadicand:glyph:position:range:)`.
    pub fn radical(
        radicand: MTDisplay,
        glyph: MTDisplay,
        position: CGPoint,
        range: NSRange,
    ) -> MTDisplay {
        let mut display = MTDisplay::base(DisplayKind::Radical(MTRadicalDisplay {
            radicand: Some(Box::new(radicand)),
            degree: None,
            radical_glyph: Some(Box::new(glyph)),
            radical_shift: 0.0,
            top_kern: 0.0,
            line_thickness: 0.0,
        }));
        display.set_position(position);
        display.range = range;
        display
    }

    /// `MTGlyphDisplay(withGlpyh:range:font:)`.
    pub fn glyph(glyph: CGGlyph, range: NSRange, font: Option<Arc<MTFont>>) -> MTDisplay {
        let mut display = MTDisplay::base(DisplayKind::Glyph(MTGlyphDisplay {
            shift_down: 0.0,
            glyph,
            font,
        }));
        display.position = zero();
        display.range = range;
        display
    }

    /// `MTGlyphConstructionDisplay(withGlyphs:offsets:font:)`. `offsets` are
    /// the `NSNumber`s' `floatValue`s.
    pub fn glyph_construction(
        glyphs: &[CGGlyph],
        offsets: &[f32],
        font: Option<Arc<MTFont>>,
    ) -> MTDisplay {
        let num_glyphs = glyphs.len();
        let mut positions = vec![zero(); num_glyphs];
        for i in 0..num_glyphs {
            positions[i] = CGPoint::new(0.0, offsets[i] as CGFloat);
        }
        let mut display =
            MTDisplay::base(DisplayKind::GlyphConstruction(MTGlyphConstructionDisplay {
                shift_down: 0.0,
                glyphs: glyphs.to_vec(),
                positions,
                font,
                num_glyphs,
            }));
        display.position = zero();
        display
    }

    /// `MTLargeOpLimitsDisplay(withNucleus:upperLimit:lowerLimit:limitShift:extraPadding:)`.
    pub fn large_op_limits(
        nucleus: MTDisplay,
        upper_limit: Option<MTDisplay>,
        lower_limit: Option<MTDisplay>,
        limit_shift: CGFloat,
        extra_padding: CGFloat,
    ) -> MTDisplay {
        let mut max_width = swift::max(
            nucleus.width(),
            upper_limit.as_ref().map_or(0.0, |l| l.width()),
        );
        max_width = swift::max(max_width, lower_limit.as_ref().map_or(0.0, |l| l.width()));
        let mut display = MTDisplay::base(DisplayKind::LargeOpLimits(MTLargeOpLimitsDisplay {
            upper_limit: upper_limit.map(Box::new),
            lower_limit: lower_limit.map(Box::new),
            nucleus: Some(Box::new(nucleus)),
            limit_shift,
            // Set in the initializer: no didSet.
            upper_limit_gap: 0.0,
            lower_limit_gap: 0.0,
            extra_padding, // corresponds to \xi_13 in TeX
        }));
        display.width = max_width;
        display
    }

    /// `MTLineDisplay(withInner:position:range:)`.
    pub fn line(inner: MTDisplay, position: CGPoint, range: NSRange) -> MTDisplay {
        let mut display = MTDisplay::base(DisplayKind::Line(MTLineDisplay {
            inner: Some(Box::new(inner)),
            line_shift_up: 0.0,
            line_thickness: 0.0,
        }));
        display.set_position(position);
        display.range = range;
        display
    }

    /// `MTAccentDisplay(withAccent:accentee:range:)`.
    pub fn accent(glyph: MTDisplay, mut accentee: MTDisplay, range: NSRange) -> MTDisplay {
        accentee.set_position(zero());
        let mut display = MTDisplay::base(DisplayKind::Accent(MTAccentDisplay {
            accent: Some(Box::new(glyph)),
            accentee: Some(Box::new(accentee)),
        }));
        display.range = range;
        display
    }

    // MARK: - Subclass accessors

    pub fn class_name(&self) -> &'static str {
        self.kind.class_name()
    }

    pub fn as_ct_line(&self) -> Option<&MTCTLineDisplay> {
        match &self.kind {
            DisplayKind::CTLine(line) => Some(line),
            _ => None,
        }
    }

    pub fn as_math_list(&self) -> Option<&MTMathListDisplay> {
        match &self.kind {
            DisplayKind::MathList(list) => Some(list),
            _ => None,
        }
    }

    pub fn as_math_list_mut(&mut self) -> Option<&mut MTMathListDisplay> {
        match &mut self.kind {
            DisplayKind::MathList(list) => Some(list),
            _ => None,
        }
    }

    pub fn as_fraction(&self) -> Option<&MTFractionDisplay> {
        match &self.kind {
            DisplayKind::Fraction(fraction) => Some(fraction),
            _ => None,
        }
    }

    pub fn as_radical(&self) -> Option<&MTRadicalDisplay> {
        match &self.kind {
            DisplayKind::Radical(radical) => Some(radical),
            _ => None,
        }
    }

    pub fn as_glyph(&self) -> Option<&MTGlyphDisplay> {
        match &self.kind {
            DisplayKind::Glyph(glyph) => Some(glyph),
            _ => None,
        }
    }

    pub fn as_glyph_construction(&self) -> Option<&MTGlyphConstructionDisplay> {
        match &self.kind {
            DisplayKind::GlyphConstruction(construction) => Some(construction),
            _ => None,
        }
    }

    pub fn as_large_op_limits(&self) -> Option<&MTLargeOpLimitsDisplay> {
        match &self.kind {
            DisplayKind::LargeOpLimits(limits) => Some(limits),
            _ => None,
        }
    }

    pub fn as_line(&self) -> Option<&MTLineDisplay> {
        match &self.kind {
            DisplayKind::Line(line) => Some(line),
            _ => None,
        }
    }

    pub fn as_accent(&self) -> Option<&MTAccentDisplay> {
        match &self.kind {
            DisplayKind::Accent(accent) => Some(accent),
            _ => None,
        }
    }

    /// `subDisplays` of an `MTMathListDisplay`.
    pub fn sub_displays(&self) -> &[MTDisplay] {
        match &self.kind {
            DisplayKind::MathList(list) => &list.sub_displays,
            _ => &[],
        }
    }

    /// `type` of an `MTMathListDisplay`.
    pub fn line_position(&self) -> LinePosition {
        self.as_math_list()
            .map_or(LinePosition::Regular, |list| list.type_)
    }

    /// `index` of an `MTMathListDisplay`.
    pub fn index(&self) -> isize {
        self.as_math_list().map_or(NSNotFound, |list| list.index)
    }

    /// `shiftDown` of an `MTDisplayDS` (glyph and glyph construction).
    pub fn shift_down(&self) -> CGFloat {
        match &self.kind {
            DisplayKind::Glyph(glyph) => glyph.shift_down,
            DisplayKind::GlyphConstruction(construction) => construction.shift_down,
            _ => 0.0,
        }
    }

    pub fn set_shift_down(&mut self, shift_down: CGFloat) {
        match &mut self.kind {
            DisplayKind::Glyph(glyph) => glyph.shift_down = shift_down,
            DisplayKind::GlyphConstruction(construction) => construction.shift_down = shift_down,
            other => panic!("{} has no shiftDown", other.class_name()),
        }
    }

    // MARK: - Overridden properties

    /// The distance from the axis to the top of the display.
    pub fn ascent(&self) -> CGFloat {
        match &self.kind {
            DisplayKind::Fraction(fraction) => {
                fraction.numerator.as_ref().unwrap().ascent() + fraction.numerator_up
            }
            DisplayKind::Glyph(glyph) => self.ascent - glyph.shift_down,
            DisplayKind::GlyphConstruction(construction) => self.ascent - construction.shift_down,
            DisplayKind::LargeOpLimits(limits) => {
                let nucleus = limits.nucleus.as_ref().unwrap();
                if let Some(upper) = &limits.upper_limit {
                    nucleus.ascent()
                        + limits.extra_padding
                        + upper.ascent()
                        + limits.upper_limit_gap
                        + upper.descent()
                } else {
                    nucleus.ascent()
                }
            }
            _ => self.ascent,
        }
    }

    /// The distance from the axis to the bottom of the display.
    pub fn descent(&self) -> CGFloat {
        match &self.kind {
            DisplayKind::Fraction(fraction) => {
                fraction.denominator.as_ref().unwrap().descent() + fraction.denominator_down
            }
            DisplayKind::Glyph(glyph) => self.descent + glyph.shift_down,
            DisplayKind::GlyphConstruction(construction) => self.descent + construction.shift_down,
            DisplayKind::LargeOpLimits(limits) => {
                let nucleus = limits.nucleus.as_ref().unwrap();
                if let Some(lower) = &limits.lower_limit {
                    nucleus.descent()
                        + limits.extra_padding
                        + limits.lower_limit_gap
                        + lower.descent()
                        + lower.ascent()
                } else {
                    nucleus.descent()
                }
            }
            _ => self.descent,
        }
    }

    /// The width of the display.
    pub fn width(&self) -> CGFloat {
        match &self.kind {
            DisplayKind::Fraction(fraction) => swift::max(
                fraction.numerator.as_ref().unwrap().width(),
                fraction.denominator.as_ref().unwrap().width(),
            ),
            _ => self.width,
        }
    }

    pub fn set_ascent(&mut self, ascent: CGFloat) {
        self.ascent = ascent;
    }

    pub fn set_descent(&mut self, descent: CGFloat) {
        self.descent = descent;
    }

    pub fn set_width(&mut self, width: CGFloat) {
        self.width = width;
    }

    /// Position of the display with respect to the parent view or display.
    pub fn position(&self) -> CGPoint {
        self.position
    }

    pub fn set_position(&mut self, position: CGPoint) {
        self.position = position;
        let (parent_position, parent_width) = (self.position, self.width());
        match &mut self.kind {
            DisplayKind::Fraction(fraction) => {
                fraction.update_denominator_position(parent_position, parent_width);
                fraction.update_numerator_position(parent_position, parent_width);
            }
            DisplayKind::Radical(radical) => radical.update_radicand_position(parent_position),
            DisplayKind::LargeOpLimits(limits) => {
                limits.update_lower_limit_position(parent_position, parent_width);
                limits.update_upper_limit_position(parent_position, parent_width);
                limits.update_nucleus_position(parent_position, parent_width);
            }
            DisplayKind::Line(line) => {
                if let Some(inner) = &mut line.inner {
                    inner.set_position(CGPoint::new(parent_position.x, parent_position.y));
                }
            }
            DisplayKind::Accent(accent) => {
                if let Some(accentee) = &mut accent.accentee {
                    accentee.set_position(CGPoint::new(parent_position.x, parent_position.y));
                }
            }
            _ => {}
        }
    }

    /// The text color for this display.
    pub fn text_color(&self) -> Option<&Retained<NSColor>> {
        self.text_color.as_ref()
    }

    pub fn set_text_color(&mut self, color: Option<Retained<NSColor>>) {
        self.text_color = color.clone();
        match &mut self.kind {
            DisplayKind::CTLine(line) => {
                let attr_str = NSMutableAttributedString::initWithAttributedString(
                    NSMutableAttributedString::alloc(),
                    &line.attributed_string,
                );
                let cg_color = self.text_color.as_ref().expect("self.textColor!").CGColor();
                unsafe {
                    attr_str.addAttribute_value_range(
                        key(kCTForegroundColorAttributeName),
                        cf_object(&*cg_color),
                        NSRange::new(0, attr_str.length()),
                    );
                }
                let attributed: Retained<NSAttributedString> = Retained::into_super(attr_str);
                // didSet: the line is rebuilt, the dimensions are not.
                line.line = create_line(&attributed);
                line.attributed_string = attributed;
            }
            DisplayKind::MathList(list) => {
                for display_atom in &mut list.sub_displays {
                    if display_atom.local_text_color.is_none() {
                        display_atom.set_text_color(color.clone());
                    } else {
                        let local = display_atom.local_text_color.clone();
                        display_atom.set_text_color(local);
                    }
                }
            }
            DisplayKind::Fraction(fraction) => {
                if let Some(numerator) = &mut fraction.numerator {
                    numerator.set_text_color(color.clone());
                }
                if let Some(denominator) = &mut fraction.denominator {
                    denominator.set_text_color(color);
                }
            }
            DisplayKind::Radical(radical) => {
                if let Some(radicand) = &mut radical.radicand {
                    radicand.set_text_color(color.clone());
                }
                if let Some(degree) = &mut radical.degree {
                    degree.set_text_color(color);
                }
            }
            DisplayKind::LargeOpLimits(limits) => {
                if let Some(upper) = &mut limits.upper_limit {
                    upper.set_text_color(color.clone());
                }
                if let Some(lower) = &mut limits.lower_limit {
                    lower.set_text_color(color.clone());
                }
                if let Some(nucleus) = &mut limits.nucleus {
                    nucleus.set_text_color(color);
                }
            }
            DisplayKind::Line(line) => {
                if let Some(inner) = &mut line.inner {
                    inner.set_text_color(color);
                }
            }
            DisplayKind::Accent(accent) => {
                if let Some(accentee) = &mut accent.accentee {
                    accentee.set_text_color(color.clone());
                }
                if let Some(glyph) = &mut accent.accent {
                    glyph.set_text_color(color);
                }
            }
            DisplayKind::Glyph(_) | DisplayKind::GlyphConstruction(_) => {}
        }
    }

    // MARK: - MTMathListDisplay

    pub fn recompute_dimensions(&mut self) {
        let DisplayKind::MathList(list) = &self.kind else {
            return;
        };
        let mut max_ascent: CGFloat = 0.0;
        let mut max_descent: CGFloat = 0.0;
        let mut max_width: CGFloat = 0.0;
        for atom in &list.sub_displays {
            let ascent = swift::max(0.0, atom.position.y + atom.ascent());
            if ascent > max_ascent {
                max_ascent = ascent;
            }

            let descent = swift::max(0.0, 0.0 - (atom.position.y - atom.descent()));
            if descent > max_descent {
                max_descent = descent;
            }
            let width = atom.width() + atom.position.x;
            if width > max_width {
                max_width = width;
            }
        }
        self.ascent = max_ascent;
        self.descent = max_descent;
        self.width = max_width;
    }

    // MARK: - MTFractionDisplay

    /// `numeratorUp`'s setter (with its didSet).
    pub fn set_numerator_up(&mut self, value: CGFloat) {
        let (position, width) = (self.position, self.width());
        if let DisplayKind::Fraction(fraction) = &mut self.kind {
            fraction.numerator_up = value;
            fraction.update_numerator_position(position, width);
        }
    }

    /// `denominatorDown`'s setter (with its didSet).
    pub fn set_denominator_down(&mut self, value: CGFloat) {
        let (position, width) = (self.position, self.width());
        if let DisplayKind::Fraction(fraction) = &mut self.kind {
            fraction.denominator_down = value;
            fraction.update_denominator_position(position, width);
        }
    }

    pub fn set_fraction_line(&mut self, line_thickness: CGFloat, line_position: CGFloat) {
        if let DisplayKind::Fraction(fraction) = &mut self.kind {
            fraction.line_thickness = line_thickness;
            fraction.line_position = line_position;
        }
    }

    // MARK: - MTRadicalDisplay

    /// `setDegree(_:fontMetrics:)`.
    pub fn set_degree(&mut self, degree: MTDisplay, font_metrics: &MTFontMathTable) {
        // sets up the degree of the radical
        let mut kern_before = font_metrics.radical_kern_before_degree();
        let kern_after = font_metrics.radical_kern_after_degree();
        let raise =
            font_metrics.radical_degree_bottom_raise_percent() * (self.ascent() - self.descent());
        let position = self.position;
        let DisplayKind::Radical(radical) = &mut self.kind else {
            panic!("setDegree on a non-radical")
        };

        // The layout is:
        // kernBefore, raise, degree, kernAfter, radical
        radical.degree = Some(Box::new(degree));

        // the radical is now shifted by kernBefore + degree.width + kernAfter
        radical.radical_shift = kern_before + radical.degree.as_ref().unwrap().width() + kern_after;
        if radical.radical_shift < 0.0 {
            // we can't have the radical shift backwards, so instead we increase the kernBefore such
            // that _radicalShift will be 0.
            kern_before -= radical.radical_shift;
            radical.radical_shift = 0.0;
        }

        // Note: position of degree is relative to parent.
        radical
            .degree
            .as_mut()
            .unwrap()
            .set_position(CGPoint::new(position.x + kern_before, position.y + raise));
        // Update the width by the _radicalShift
        let width = radical.radical_shift
            + radical.radical_glyph.as_ref().unwrap().width()
            + radical.radicand.as_ref().unwrap().width();
        self.width = width;
        // update the position of the radicand
        if let DisplayKind::Radical(radical) = &mut self.kind {
            radical.update_radicand_position(position);
        }
    }

    pub fn set_radical_metrics(&mut self, top_kern: CGFloat, line_thickness: CGFloat) {
        if let DisplayKind::Radical(radical) = &mut self.kind {
            radical.top_kern = top_kern;
            radical.line_thickness = line_thickness;
        }
    }

    // MARK: - MTLargeOpLimitsDisplay

    /// `upperLimitGap`'s setter (with its didSet).
    pub fn set_upper_limit_gap(&mut self, gap: CGFloat) {
        let (position, width) = (self.position, self.width());
        if let DisplayKind::LargeOpLimits(limits) = &mut self.kind {
            limits.upper_limit_gap = gap;
            limits.update_upper_limit_position(position, width);
        }
    }

    /// `lowerLimitGap`'s setter (with its didSet).
    pub fn set_lower_limit_gap(&mut self, gap: CGFloat) {
        let (position, width) = (self.position, self.width());
        if let DisplayKind::LargeOpLimits(limits) = &mut self.kind {
            limits.lower_limit_gap = gap;
            limits.update_lower_limit_position(position, width);
        }
    }

    // MARK: - MTLineDisplay

    pub fn set_line_metrics(&mut self, line_shift_up: CGFloat, line_thickness: CGFloat) {
        if let DisplayKind::Line(line) = &mut self.kind {
            line.line_shift_up = line_shift_up;
            line.line_thickness = line_thickness;
        }
    }

    // MARK: - Drawing

    /// Gets the bounding rectangle for the MTDisplay.
    pub fn display_bounds(&self) -> CGRect {
        CGRect::new(
            CGPoint::new(self.position.x, self.position.y - self.descent()),
            CGSize::new(self.width(), self.ascent() + self.descent()),
        )
    }

    /// Draws itself in the given graphics context.
    pub fn draw(&self, context: &CGContext) {
        let ctx = Some(context);
        // MTDisplay.draw: the local background.
        if let Some(background) = &self.local_background_color {
            CGContext::save_g_state(ctx);
            CGContext::set_blend_mode(ctx, CGBlendMode::Normal);
            CGContext::set_fill_color_with_color(ctx, Some(&background.CGColor()));
            CGContext::fill_rect(ctx, self.display_bounds());
            CGContext::restore_g_state(ctx);
        }
        match &self.kind {
            DisplayKind::CTLine(line) => {
                CGContext::save_g_state(ctx);
                CGContext::set_text_position(ctx, self.position.x, self.position.y);
                unsafe { line.line.draw(context) };
                CGContext::restore_g_state(ctx);
            }
            DisplayKind::MathList(list) => {
                CGContext::save_g_state(ctx);
                // Make the current position the origin as all the positions of the sub atoms are relative to the origin.
                CGContext::translate_ctm(ctx, self.position.x, self.position.y);
                CGContext::set_text_position(ctx, 0.0, 0.0);
                // draw each atom separately
                for display_atom in &list.sub_displays {
                    display_atom.draw(context);
                }
                CGContext::restore_g_state(ctx);
            }
            DisplayKind::Fraction(fraction) => {
                if let Some(numerator) = &fraction.numerator {
                    numerator.draw(context);
                }
                if let Some(denominator) = &fraction.denominator {
                    denominator.draw(context);
                }

                CGContext::save_g_state(ctx);
                if let Some(color) = &self.text_color {
                    color.setStroke();
                }
                // draw the horizontal line
                // Note: line thickness of 0 draws the thinnest possible line - we want no line so check for 0s
                if fraction.line_thickness > 0.0 {
                    let path = NSBezierPath::new();
                    path.moveToPoint(CGPoint::new(
                        self.position.x,
                        self.position.y + fraction.line_position,
                    ));
                    path.lineToPoint(CGPoint::new(
                        self.position.x + self.width(),
                        self.position.y + fraction.line_position,
                    ));
                    path.setLineWidth(fraction.line_thickness);
                    path.stroke();
                }
                CGContext::restore_g_state(ctx);
            }
            DisplayKind::Radical(radical) => {
                // draw the radicand & degree at its position
                if let Some(radicand) = &radical.radicand {
                    radicand.draw(context);
                }
                if let Some(degree) = &radical.degree {
                    degree.draw(context);
                }

                CGContext::save_g_state(ctx);
                if let Some(color) = &self.text_color {
                    color.setStroke();
                }
                if let Some(color) = &self.text_color {
                    color.setFill();
                }

                // Make the current position the origin as all the positions of the sub atoms are relative to the origin.
                CGContext::translate_ctm(
                    ctx,
                    self.position.x + radical.radical_shift,
                    self.position.y,
                );
                CGContext::set_text_position(ctx, 0.0, 0.0);

                // Draw the glyph.
                if let Some(glyph) = &radical.radical_glyph {
                    glyph.draw(context);
                }

                // Draw the VBOX
                // for the kern of, we don't need to draw anything.
                let height_from_top = radical.top_kern;

                // draw the horizontal line with the given thickness
                let path = NSBezierPath::new();
                let glyph_width = radical.radical_glyph.as_ref().unwrap().width();
                // subtract half the line thickness to center the line
                let line_start = CGPoint::new(
                    glyph_width,
                    self.ascent() - height_from_top - radical.line_thickness / 2.0,
                );
                let line_end = CGPoint::new(
                    line_start.x + radical.radicand.as_ref().unwrap().width(),
                    line_start.y,
                );
                path.moveToPoint(line_start);
                path.lineToPoint(line_end);
                path.setLineWidth(radical.line_thickness);
                path.setLineCapStyle(NSLineCapStyle::Round);
                path.stroke();

                CGContext::restore_g_state(ctx);
            }
            DisplayKind::Glyph(glyph) => {
                CGContext::save_g_state(ctx);

                if let Some(color) = &self.text_color {
                    color.setFill();
                }

                // Make the current position the origin as all the positions of the sub atoms are relative to the origin.
                CGContext::translate_ctm(ctx, self.position.x, self.position.y - glyph.shift_down);
                CGContext::set_text_position(ctx, 0.0, 0.0);

                let mut glyph_id = glyph.glyph;
                let mut pos = zero();
                unsafe {
                    glyph.font.as_ref().expect("font!").ct_font().draw_glyphs(
                        std::ptr::NonNull::from(&mut glyph_id),
                        std::ptr::NonNull::from(&mut pos),
                        1,
                        context,
                    );
                }

                CGContext::restore_g_state(ctx);
            }
            DisplayKind::GlyphConstruction(construction) => {
                CGContext::save_g_state(ctx);

                if let Some(color) = &self.text_color {
                    color.setFill();
                }

                // Make the current position the origin as all the positions of the sub atoms are relative to the origin.
                CGContext::translate_ctm(
                    ctx,
                    self.position.x,
                    self.position.y - construction.shift_down,
                );
                CGContext::set_text_position(ctx, 0.0, 0.0);

                // Draw the glyphs.
                let mut glyphs = construction.glyphs.clone();
                let mut positions = construction.positions.clone();
                if let (Some(glyphs), Some(positions)) = (
                    std::ptr::NonNull::new(glyphs.as_mut_ptr()),
                    std::ptr::NonNull::new(positions.as_mut_ptr()),
                ) {
                    unsafe {
                        construction
                            .font
                            .as_ref()
                            .expect("font!")
                            .ct_font()
                            .draw_glyphs(glyphs, positions, construction.num_glyphs, context);
                    }
                }

                CGContext::restore_g_state(ctx);
            }
            DisplayKind::LargeOpLimits(limits) => {
                // Draw the elements.
                if let Some(upper) = &limits.upper_limit {
                    upper.draw(context);
                }
                if let Some(lower) = &limits.lower_limit {
                    lower.draw(context);
                }
                if let Some(nucleus) = &limits.nucleus {
                    nucleus.draw(context);
                }
            }
            DisplayKind::Line(line) => {
                if let Some(inner) = &line.inner {
                    inner.draw(context);
                }

                CGContext::save_g_state(ctx);

                if let Some(color) = &self.text_color {
                    color.setStroke();
                }

                // draw the horizontal line
                let path = NSBezierPath::new();
                let line_start =
                    CGPoint::new(self.position.x, self.position.y + line.line_shift_up);
                let line_end = CGPoint::new(
                    line_start.x + line.inner.as_ref().unwrap().width(),
                    line_start.y,
                );
                path.moveToPoint(line_start);
                path.lineToPoint(line_end);
                path.setLineWidth(line.line_thickness);
                path.stroke();

                CGContext::restore_g_state(ctx);
            }
            DisplayKind::Accent(accent) => {
                if let Some(accentee) = &accent.accentee {
                    accentee.draw(context);
                }

                CGContext::save_g_state(ctx);
                CGContext::translate_ctm(ctx, self.position.x, self.position.y);
                CGContext::set_text_position(ctx, 0.0, 0.0);

                if let Some(glyph) = &accent.accent {
                    glyph.draw(context);
                }

                CGContext::restore_g_state(ctx);
            }
        }
    }

    /// Drops the atoms the lines keep for indexing, which only the model
    /// side reads; what is left is plain Core Foundation and AppKit objects.
    pub(crate) fn strip_atoms(&mut self) {
        match &mut self.kind {
            DisplayKind::CTLine(line) => line.atoms.clear(),
            DisplayKind::MathList(list) => list
                .sub_displays
                .iter_mut()
                .for_each(MTDisplay::strip_atoms),
            DisplayKind::Fraction(fraction) => {
                fraction.numerator.iter_mut().for_each(|d| d.strip_atoms());
                fraction
                    .denominator
                    .iter_mut()
                    .for_each(|d| d.strip_atoms());
            }
            DisplayKind::Radical(radical) => {
                radical.radicand.iter_mut().for_each(|d| d.strip_atoms());
                radical.degree.iter_mut().for_each(|d| d.strip_atoms());
                radical
                    .radical_glyph
                    .iter_mut()
                    .for_each(|d| d.strip_atoms());
            }
            DisplayKind::LargeOpLimits(limits) => {
                limits.upper_limit.iter_mut().for_each(|d| d.strip_atoms());
                limits.lower_limit.iter_mut().for_each(|d| d.strip_atoms());
                limits.nucleus.iter_mut().for_each(|d| d.strip_atoms());
            }
            DisplayKind::Line(line) => line.inner.iter_mut().for_each(|d| d.strip_atoms()),
            DisplayKind::Accent(accent) => {
                accent.accentee.iter_mut().for_each(|d| d.strip_atoms());
                accent.accent.iter_mut().for_each(|d| d.strip_atoms());
            }
            DisplayKind::Glyph(_) | DisplayKind::GlyphConstruction(_) => {}
        }
    }
}

impl MTFractionDisplay {
    fn update_denominator_position(&mut self, position: CGPoint, width: CGFloat) {
        let denominator_down = self.denominator_down;
        let Some(denominator) = &mut self.denominator else {
            return;
        };
        let x = position.x + (width - denominator.width()) / 2.0;
        denominator.set_position(CGPoint::new(x, position.y - denominator_down));
    }

    fn update_numerator_position(&mut self, position: CGPoint, width: CGFloat) {
        let numerator_up = self.numerator_up;
        let Some(numerator) = &mut self.numerator else {
            return;
        };
        let x = position.x + (width - numerator.width()) / 2.0;
        numerator.set_position(CGPoint::new(x, position.y + numerator_up));
    }
}

impl MTRadicalDisplay {
    fn update_radicand_position(&mut self, position: CGPoint) {
        // The position of the radicand includes the position of the MTRadicalDisplay
        // move the radicand by the width of the radical sign
        let glyph_width = self.radical_glyph.as_ref().unwrap().width();
        let shift = self.radical_shift;
        self.radicand
            .as_mut()
            .unwrap()
            .set_position(CGPoint::new(position.x + shift + glyph_width, position.y));
    }
}

impl MTLargeOpLimitsDisplay {
    fn update_lower_limit_position(&mut self, position: CGPoint, width: CGFloat) {
        let nucleus_descent = self.nucleus.as_ref().unwrap().descent();
        let (limit_shift, gap) = (self.limit_shift, self.lower_limit_gap);
        if let Some(lower) = &mut self.lower_limit {
            // Move the starting point to below the nucleus leaving a gap of _lowerLimitGap and subtract
            // the ascent to to get the baseline. Also center and shift it to the left by _limitShift.
            let x = position.x - limit_shift + (width - lower.width()) / 2.0;
            let y = position.y - nucleus_descent - gap - lower.ascent();
            lower.set_position(CGPoint::new(x, y));
        }
    }

    fn update_upper_limit_position(&mut self, position: CGPoint, width: CGFloat) {
        let nucleus_ascent = self.nucleus.as_ref().unwrap().ascent();
        let (limit_shift, gap) = (self.limit_shift, self.upper_limit_gap);
        if let Some(upper) = &mut self.upper_limit {
            // Move the starting point to above the nucleus leaving a gap of _upperLimitGap and add
            // the descent to to get the baseline. Also center and shift it to the right by _limitShift.
            let x = position.x + limit_shift + (width - upper.width()) / 2.0;
            let y = position.y + nucleus_ascent + gap + upper.descent();
            upper.set_position(CGPoint::new(x, y));
        }
    }

    fn update_nucleus_position(&mut self, position: CGPoint, width: CGFloat) {
        // Center the nucleus
        if let Some(nucleus) = &mut self.nucleus {
            let x = position.x + (width - nucleus.width()) / 2.0;
            nucleus.set_position(CGPoint::new(x, position.y));
        }
    }
}

/// `CTLineCreateWithAttributedString`.
pub(crate) fn create_line(string: &NSAttributedString) -> CFRetained<CTLine> {
    // NSAttributedString is toll-free bridged to CFAttributedString.
    let cf = unsafe { &*(string as *const NSAttributedString as *const CFAttributedString) };
    unsafe { CTLine::with_attributed_string(cf) }
}

/// A Core Text attribute name as an `NSAttributedString.Key`.
pub(crate) fn key(name: &CFString) -> &NSString {
    // CFString is toll-free bridged to NSString.
    unsafe { &*(name as *const CFString as *const NSString) }
}

/// A Core Foundation object as an Objective-C object.
pub(crate) fn cf_object<T: ?Sized>(object: &T) -> &AnyObject {
    unsafe { &*(object as *const T as *const AnyObject) }
}
