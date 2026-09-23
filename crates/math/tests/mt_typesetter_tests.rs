//! Port of `Tests/SwiftMathTests/MTTypesetterTests.swift`
//! (`vendor/downright/Vendor/SwiftMath`): every test, every assertion, in the
//! same order and with the same accuracies.
//!
//! - `XCTAssertEqual(a, b, accuracy: e)` is `(a - b).abs() <= e`.
//! - `CGPointEqualToPoint` is exact equality of both coordinates.
//! - The file's `CGPoint.isEqual(to:accuracy:)` uses a strict `<`.
//! - `XCTAssertNotNil(display)` right after a force unwrap of
//!   `createLineForMathList` is implied by the `unwrap()` in [`typeset`].
//! - `testAtomWithAllFontStyles(_:)` takes an argument, so XCTest never runs
//!   it on its own; it is a helper here too.

use std::sync::Arc;

use objc2_core_foundation::CGPoint;
use objc2_foundation::{NSNotFound, NSRange};
use upleft_math::math_render::mt_font::MTFont;
use upleft_math::math_render::mt_font_manager::MTFontManager;
use upleft_math::math_render::mt_math_atom_factory::MTMathAtomFactory;
use upleft_math::math_render::mt_math_list::{
    MTColumnAlignment, MTFontStyle, MTLineStyle, MTMathAtom, MTMathAtomRef, MTMathAtomType,
    MTMathList, MTMathListRef,
};
use upleft_math::math_render::mt_math_list_builder::MTMathListBuilder;
use upleft_math::math_render::mt_math_list_display::{
    LinePosition, MTCTLineDisplay, MTDisplay, NS_NOT_FOUND,
};
use upleft_math::math_render::mt_typesetter::MTTypesetter;
use upleft_math::swift;

// MARK: - Helpers

/// `setUpWithError`: `MTFontManager.fontManager.defaultFont`.
fn default_font() -> Arc<MTFont> {
    MTFontManager::font_manager().default_font().unwrap()
}

/// `MTTypesetter.createLineForMathList(list, font:, style:)!`.
fn typeset(list: Option<&MTMathListRef>, font: &Arc<MTFont>, style: MTLineStyle) -> MTDisplay {
    MTTypesetter::create_line_for_math_list(list, font, style).unwrap()
}

/// `CGPointMake(x, y)`.
fn point(x: f64, y: f64) -> CGPoint {
    CGPoint::new(x, y)
}

/// `CGPointZero`.
fn zero() -> CGPoint {
    CGPoint::new(0.0, 0.0)
}

/// `CGPointEqualToPoint(p1, p2)`.
fn point_equal(p1: CGPoint, p2: CGPoint) -> bool {
    p1.x == p2.x && p1.y == p2.y
}

/// `extension CGPoint { func isEqual(to:accuracy:) }` from the Swift test file.
fn is_equal(this: CGPoint, p: CGPoint, accuracy: f64) -> bool {
    (this.x - p.x).abs() < accuracy && (this.y - p.y).abs() < accuracy
}

/// `NSMakeRange(location, length)`.
fn make_range(location: usize, length: usize) -> NSRange {
    NSRange::new(location, length)
}

/// `NSEqualRanges(r1, r2)`.
fn equal_ranges(r1: NSRange, r2: NSRange) -> bool {
    r1.location == r2.location && r1.length == r2.length
}

/// `sub as! MTCTLineDisplay`.
fn ct(display: &MTDisplay) -> &MTCTLineDisplay {
    display.as_ct_line().unwrap()
}

/// `line.attributedString?.string`.
fn string_of(display: &MTDisplay) -> String {
    ct(display).attributed_string.string().to_string()
}

/// `XCTAssertEqual(a, b, accuracy: e)`.
macro_rules! assert_accuracy {
    ($a:expr, $b:expr, $accuracy:expr) => {{
        let (a, b, accuracy): (f64, f64, f64) = ($a, $b, $accuracy);
        assert!(
            (a - b).abs() <= accuracy,
            "{} = {a} is not {b} ± {accuracy}",
            stringify!($a)
        );
    }};
}

// MARK: - Tests

#[test]
fn test_simple_variable() {
    let font = default_font();
    let math_list = MTMathList::new();
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));
    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    // The x is italicized
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 1)));
    assert!(!line.has_script);

    // dimensions
    assert_eq!(display.ascent(), line.ascent());
    assert_eq!(display.descent(), line.descent());
    assert_eq!(display.width(), line.width());

    assert_accuracy!(display.ascent(), 8.834, 0.01);
    assert_accuracy!(display.descent(), 0.22, 0.01);
    assert_accuracy!(display.width(), 11.44, 0.01);
}

#[test]
fn test_multiple_variables() {
    let font = default_font();
    let math_list = MTMathAtomFactory::math_list_for_characters("xyzw");
    let display = typeset(math_list.as_ref(), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 4)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 4);
    assert_eq!(string_of(line), "𝑥𝑦𝑧𝑤");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 4)));
    assert!(!line.has_script);

    // dimensions
    assert_eq!(display.ascent(), line.ascent());
    assert_eq!(display.descent(), line.descent());
    assert_eq!(display.width(), line.width());

    assert_accuracy!(display.ascent(), 8.834, 0.01);
    assert_accuracy!(display.descent(), 4.10, 0.01);
    assert_accuracy!(display.width(), 44.86, 0.01);
}

#[test]
fn test_variables_and_numbers() {
    let font = default_font();
    let math_list = MTMathAtomFactory::math_list_for_characters("xy2w");
    let display = typeset(math_list.as_ref(), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 4)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 4);
    assert_eq!(string_of(line), "𝑥𝑦2𝑤");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 4)));
    assert!(!line.has_script);

    // dimensions
    assert_eq!(display.ascent(), line.ascent());
    assert_eq!(display.descent(), line.descent());
    assert_eq!(display.width(), line.width());

    assert_accuracy!(display.ascent(), 13.32, 0.01);
    assert_accuracy!(display.descent(), 4.10, 0.01);
    assert_accuracy!(display.width(), 45.56, 0.01);
}

#[test]
fn test_equation_with_operators_and_relations() {
    let font = default_font();
    let math_list = MTMathAtomFactory::math_list_for_characters("2x+3=y");
    let display = typeset(math_list.as_ref(), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 6)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 6);
    assert_eq!(string_of(line), "2𝑥+3=𝑦");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 6)));
    assert!(!line.has_script);

    // dimensions
    assert_eq!(display.ascent(), line.ascent());
    assert_eq!(display.descent(), line.descent());
    assert_eq!(display.width(), line.width());

    assert_accuracy!(display.ascent(), 13.32, 0.01);
    assert_accuracy!(display.descent(), 4.10, 0.01);
    assert_accuracy!(display.width(), 92.36, 0.01);
}

//    #define XCTAssertTrue(CGPointEqualToPoint(p1, p2, accuracy, ...) \
//        XCTAssertEqual(p1.x, p2.x, accuracy, __VA_ARGS__); \
//        XCTAssertEqual(p1.y, p2.y, accuracy, __VA_ARGS__)
//
//
//    #define XCTAssertTrue(NSEqualRanges(r1, r2, ...) \
//        XCTAssertEqual(r1.location, r2.location, __VA_ARGS__); \
//        XCTAssertEqual(r1.length, r2.length, __VA_ARGS__)

#[test]
fn test_superscript() {
    let font = default_font();
    let math_list = MTMathList::new();
    let x = MTMathAtomFactory::atom_for_character("x");
    let supersc = MTMathList::new();
    supersc
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("2"));
    if let Some(x) = &x {
        x.borrow_mut().set_super_script(Some(supersc));
    }
    math_list.borrow_mut().add(x);

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    // The x is italicized
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(line.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTMathListDisplay");
    let display2 = sub1;
    assert_eq!(display2.line_position(), LinePosition::Superscript);
    assert!(
        point_equal(display2.position(), point(11.44, 7.26)),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), 0);
    assert_eq!(display2.sub_displays().len(), 1);

    let sub1sub0 = &display2.sub_displays()[0];
    assert_eq!(sub1sub0.class_name(), "MTCTLineDisplay");
    let line2 = sub1sub0;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "2");
    assert!(point_equal(line2.position(), zero()));
    assert!(!line2.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 16.584, 0.01);
    assert_accuracy!(display.descent(), 0.22, 0.01);
    assert_accuracy!(display.width(), 18.44, 0.01);
}

#[test]
fn test_subscript() {
    let font = default_font();
    let math_list = MTMathList::new();
    let x = MTMathAtomFactory::atom_for_character("x");
    let subsc = MTMathList::new();
    subsc
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    if let Some(x) = &x {
        x.borrow_mut().set_sub_script(Some(subsc));
    }
    math_list.borrow_mut().add(x);

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    // The x is italicized
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(line.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTMathListDisplay");
    let display2 = sub1;
    assert_eq!(display2.line_position(), LinePosition::Subscript);
    assert!(
        point_equal(display2.position(), point(11.44, -4.94)),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), 0);
    assert_eq!(display2.sub_displays().len(), 1);

    let sub1sub0 = &display2.sub_displays()[0];
    assert_eq!(sub1sub0.class_name(), "MTCTLineDisplay");
    let line2 = sub1sub0;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(!line2.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 8.834, 0.01);
    assert_accuracy!(display.descent(), 4.940, 0.01);
    assert_accuracy!(display.width(), 18.44, 0.01);
}

#[test]
fn test_supersubscript() {
    let font = default_font();
    let math_list = MTMathList::new();
    let x = MTMathAtomFactory::atom_for_character("x");
    let supersc = MTMathList::new();
    supersc
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("2"));
    let subsc = MTMathList::new();
    subsc
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    if let Some(x) = &x {
        x.borrow_mut().set_sub_script(Some(subsc));
    }
    if let Some(x) = &x {
        x.borrow_mut().set_super_script(Some(supersc));
    }
    math_list.borrow_mut().add(x);

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 3);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    // The x is italicized
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(line.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTMathListDisplay");
    let display2 = sub1;
    assert_eq!(display2.line_position(), LinePosition::Superscript);
    assert!(
        point_equal(display2.position(), point(11.44, 7.26)),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), 0);
    assert_eq!(display2.sub_displays().len(), 1);

    let sub1sub0 = &display2.sub_displays()[0];
    assert_eq!(sub1sub0.class_name(), "MTCTLineDisplay");
    let line2 = sub1sub0;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "2");
    assert!(point_equal(line2.position(), zero()));
    assert!(!line2.has_script);

    let sub2 = &display.sub_displays()[2];
    assert_eq!(sub2.class_name(), "MTMathListDisplay");
    let display3 = sub2;
    assert_eq!(display3.line_position(), LinePosition::Subscript);
    // Positioned differently when both subscript and superscript present.
    assert!(
        point_equal(display3.position(), point(11.44, -5.264)),
        "Got {:?}",
        display3.position()
    );
    assert!(equal_ranges(display3.range, make_range(0, 1)));
    assert!(!display3.has_script);
    assert_eq!(display3.index(), 0);
    assert_eq!(display3.sub_displays().len(), 1);

    let sub2sub0 = &display3.sub_displays()[0];
    assert_eq!(sub2sub0.class_name(), "MTCTLineDisplay");
    let line3 = sub2sub0;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "1");
    assert!(point_equal(line3.position(), zero()));
    assert!(!line3.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 16.584, 0.01);
    assert_accuracy!(display.descent(), 5.264, 0.01);
    assert_accuracy!(display.width(), 18.44, 0.01);
}

#[test]
fn test_radical() {
    let font = default_font();
    let math_list = MTMathList::new();
    let rad = MTMathAtom::radical();
    let radicand = MTMathList::new();
    radicand
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    rad.borrow_mut().as_radical_mut().unwrap().radicand = Some(radicand);
    math_list.borrow_mut().add(Some(rad));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTRadicalDisplay");
    let radical = sub0.as_radical().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(radical.radicand.is_some());
    assert!(radical.degree.is_none());

    let display2 = radical.radicand.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        is_equal(point(16.66, 0.0), display2.position(), 0.01),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subrad = &display2.sub_displays()[0];
    assert_eq!(subrad.class_name(), "MTCTLineDisplay");
    let line2 = subrad;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 19.34, 0.01);
    assert_accuracy!(display.descent(), 1.46, 0.01);
    assert_accuracy!(display.width(), 26.66, 0.01);
}

#[test]
fn test_radical_with_degree() {
    let font = default_font();
    let math_list = MTMathList::new();
    let rad = MTMathAtom::radical();
    let radicand = MTMathList::new();
    radicand
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    let degree = MTMathList::new();
    degree
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("3"));
    rad.borrow_mut().as_radical_mut().unwrap().radicand = Some(radicand);
    rad.borrow_mut().as_radical_mut().unwrap().degree = Some(degree);
    math_list.borrow_mut().add(Some(rad));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTRadicalDisplay");
    let radical = sub0.as_radical().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(radical.radicand.is_some());
    assert!(radical.degree.is_some());

    let display2 = radical.radicand.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), point(16.66, 0.0)),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subrad = &display2.sub_displays()[0];
    assert_eq!(subrad.class_name(), "MTCTLineDisplay");
    let line2 = subrad;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    let display3 = radical.degree.as_deref().unwrap();
    assert_eq!(display3.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display3.position(), point(6.12, 10.728)),
        "Got {:?}",
        display3.position()
    );
    assert!(equal_ranges(display3.range, make_range(0, 1)));
    assert!(!display3.has_script);
    assert_eq!(display3.index(), NSNotFound);
    assert_eq!(display3.sub_displays().len(), 1);

    let subdeg = &display3.sub_displays()[0];
    assert_eq!(subdeg.class_name(), "MTCTLineDisplay");
    let line3 = subdeg;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "3");
    assert!(point_equal(line3.position(), zero()));
    assert!(equal_ranges(line3.range, make_range(0, 1)));
    assert!(!line3.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 19.34, 0.01);
    assert_accuracy!(display.descent(), 1.46, 0.01);
    assert_accuracy!(display.width(), 26.66, 0.01);
}

#[test]
fn test_fraction() {
    let font = default_font();
    let math_list = MTMathList::new();
    let frac = MTMathAtom::fraction(true);
    let num = MTMathList::new();
    num.borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    let denom = MTMathList::new();
    denom
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("3"));
    frac.borrow_mut().as_fraction_mut().unwrap().numerator = Some(num);
    frac.borrow_mut().as_fraction_mut().unwrap().denominator = Some(denom);
    math_list.borrow_mut().add(Some(frac));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTFractionDisplay");
    let fraction = sub0.as_fraction().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(fraction.numerator.is_some());
    assert!(fraction.denominator.is_some());

    let display2 = fraction.numerator.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), point(0.0, 13.54)),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subnum = &display2.sub_displays()[0];
    assert_eq!(subnum.class_name(), "MTCTLineDisplay");
    let line2 = subnum;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    let display3 = fraction.denominator.as_deref().unwrap();
    assert_eq!(display3.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display3.position(), point(0.0, -13.72)),
        "Got {:?}",
        display3.position()
    );
    assert!(equal_ranges(display3.range, make_range(0, 1)));
    assert!(!display3.has_script);
    assert_eq!(display3.index(), NSNotFound);
    assert_eq!(display3.sub_displays().len(), 1);

    let subdenom = &display3.sub_displays()[0];
    assert_eq!(subdenom.class_name(), "MTCTLineDisplay");
    let line3 = subdenom;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "3");
    assert!(point_equal(line3.position(), zero()));
    assert!(equal_ranges(line3.range, make_range(0, 1)));
    assert!(!line3.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 26.86, 0.01);
    assert_accuracy!(display.descent(), 14.16, 0.01);
    assert_accuracy!(display.width(), 10.0, 0.01);
}

#[test]
fn test_atop() {
    let font = default_font();
    let math_list = MTMathList::new();
    let frac = MTMathAtom::fraction(false);
    let num = MTMathList::new();
    num.borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    let denom = MTMathList::new();
    denom
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("3"));
    frac.borrow_mut().as_fraction_mut().unwrap().numerator = Some(num);
    frac.borrow_mut().as_fraction_mut().unwrap().denominator = Some(denom);
    math_list.borrow_mut().add(Some(frac));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTFractionDisplay");
    let fraction = sub0.as_fraction().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(fraction.numerator.is_some());
    assert!(fraction.denominator.is_some());

    let display2 = fraction.numerator.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), point(0.0, 13.54)),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subnum = &display2.sub_displays()[0];
    assert_eq!(subnum.class_name(), "MTCTLineDisplay");
    let line2 = subnum;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    let display3 = fraction.denominator.as_deref().unwrap();
    assert_eq!(display3.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display3.position(), point(0.0, -13.72)),
        "Got {:?}",
        display3.position()
    );
    assert!(equal_ranges(display3.range, make_range(0, 1)));
    assert!(!display3.has_script);
    assert_eq!(display3.index(), NSNotFound);
    assert_eq!(display3.sub_displays().len(), 1);

    let subdenom = &display3.sub_displays()[0];
    assert_eq!(subdenom.class_name(), "MTCTLineDisplay");
    let line3 = subdenom;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "3");
    assert!(point_equal(line3.position(), zero()));
    assert!(equal_ranges(line3.range, make_range(0, 1)));
    assert!(!line3.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 26.86, 0.01);
    assert_accuracy!(display.descent(), 14.16, 0.01);
    assert_accuracy!(display.width(), 10.0, 0.01);
}

#[test]
fn test_binomial() {
    let font = default_font();
    let math_list = MTMathList::new();
    let frac = MTMathAtom::fraction(false);
    let num = MTMathList::new();
    num.borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    let denom = MTMathList::new();
    denom
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("3"));
    {
        let mut atom = frac.borrow_mut();
        let f = atom.as_fraction_mut().unwrap();
        f.numerator = Some(num);
        f.denominator = Some(denom);
        f.left_delimiter = "(".to_owned();
        f.right_delimiter = ")".to_owned();
    }
    math_list.borrow_mut().add(Some(frac));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTMathListDisplay");
    let display0 = sub0;
    assert_eq!(display0.line_position(), LinePosition::Regular);
    assert!(point_equal(display0.position(), zero()));
    assert!(equal_ranges(display0.range, make_range(0, 1)));
    assert!(!display0.has_script);
    assert_eq!(display0.index(), NSNotFound);
    assert_eq!(display0.sub_displays().len(), 3);

    let sub_left = &display0.sub_displays()[0];
    assert_eq!(sub_left.class_name(), "MTGlyphDisplay");
    let glyph = sub_left;
    assert!(point_equal(glyph.position(), zero()));
    assert!(equal_ranges(glyph.range, make_range(NS_NOT_FOUND, 0)));
    assert!(!glyph.has_script);

    let sub_frac = &display0.sub_displays()[1];
    assert_eq!(sub_frac.class_name(), "MTFractionDisplay");
    let fraction = sub_frac.as_fraction().unwrap();
    assert!(equal_ranges(sub_frac.range, make_range(0, 1)));
    assert!(!sub_frac.has_script);
    assert!(
        point_equal(sub_frac.position(), point(14.72, 0.0)),
        "Got {:?}",
        sub_frac.position()
    );
    assert!(fraction.numerator.is_some());
    assert!(fraction.denominator.is_some());

    let display2 = fraction.numerator.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        is_equal(point(14.72, 13.54), display2.position(), 0.01),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subnum = &display2.sub_displays()[0];
    assert_eq!(subnum.class_name(), "MTCTLineDisplay");
    let line2 = subnum;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    let display3 = fraction.denominator.as_deref().unwrap();
    assert_eq!(display3.line_position(), LinePosition::Regular);
    assert!(
        is_equal(point(14.72, -13.72), display3.position(), 0.01),
        "Got {:?}",
        display3.position()
    );
    assert!(equal_ranges(display3.range, make_range(0, 1)));
    assert!(!display3.has_script);
    assert_eq!(display3.index(), NSNotFound);
    assert_eq!(display3.sub_displays().len(), 1);

    let subdenom = &display3.sub_displays()[0];
    assert_eq!(subdenom.class_name(), "MTCTLineDisplay");
    let line3 = subdenom;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "3");
    assert!(point_equal(line3.position(), zero()));
    assert!(equal_ranges(line3.range, make_range(0, 1)));
    assert!(!line3.has_script);

    let sub_right = &display0.sub_displays()[2];
    assert_eq!(sub_right.class_name(), "MTGlyphDisplay");
    let glyph2 = sub_right;
    assert!(
        point_equal(glyph2.position(), point(24.72, 0.0)),
        "Got {:?}",
        glyph2.position()
    );
    assert!(
        equal_ranges(glyph2.range, make_range(NS_NOT_FOUND, 0)),
        "Got {:?} instead",
        glyph2.range
    );
    assert!(!glyph2.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 28.92, 0.001);
    assert_accuracy!(display.descent(), 18.92, 0.001);
    assert_accuracy!(display.width(), 39.44, 0.001);
}

#[test]
fn test_large_op_no_limits_text() {
    let font = default_font();
    let math_list = MTMathList::new();
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_latex_symbol("sin"));
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 2)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    assert_eq!(string_of(line), "sin");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 1)));
    assert!(!line.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTCTLineDisplay");
    let line2 = sub1;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑥");
    assert!(
        is_equal(point(27.893, 0.0), line2.position(), 0.01),
        "Got {:?}",
        line2.position()
    );
    assert!(
        equal_ranges(line2.range, make_range(1, 1)),
        "Got {:?} instead",
        line2.range
    );
    assert!(!line2.has_script);

    assert_accuracy!(display.ascent(), 13.14, 0.01);
    assert_accuracy!(display.descent(), 0.22, 0.01);
    assert_accuracy!(display.width(), 39.33, 0.01);
}

#[test]
fn test_large_op_no_limits_symbol() {
    let font = default_font();
    let math_list = MTMathList::new();
    // Integral
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_latex_symbol("int"));
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 2)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTGlyphDisplay");
    let glyph = sub0;
    assert!(point_equal(glyph.position(), zero()));
    assert!(equal_ranges(glyph.range, make_range(0, 1)));
    assert!(!glyph.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTCTLineDisplay");
    let line2 = sub1;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑥");
    assert!(
        is_equal(point(23.313, 0.0), line2.position(), 0.01),
        "Got {:?}",
        line2.position()
    );
    assert!(
        equal_ranges(line2.range, make_range(1, 1)),
        "Got {:?} instead",
        line2.range
    );
    assert!(!line2.has_script);

    assert_accuracy!(display.ascent(), 27.22, 0.01);
    assert_accuracy!(display.descent(), 17.22, 0.01);
    assert_accuracy!(display.width(), 34.753, 0.01);
}

#[test]
fn test_large_op_no_limits_symbol_with_scripts() {
    let font = default_font();
    let math_list = MTMathList::new();
    // Integral
    let op = MTMathAtomFactory::atom_for_latex_symbol("int").unwrap();
    op.borrow_mut().set_super_script(Some(MTMathList::new()));
    op.borrow()
        .super_script()
        .unwrap()
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    op.borrow_mut().set_sub_script(Some(MTMathList::new()));
    op.borrow()
        .sub_script()
        .unwrap()
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("0"));
    math_list.borrow_mut().add(Some(op));
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 2)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 4);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTMathListDisplay");
    let display0 = sub0;
    assert_eq!(display0.line_position(), LinePosition::Superscript);
    assert!(
        point_equal(display0.position(), point(19.98, 23.72)),
        "Got {:?}",
        display0.position()
    );
    assert!(equal_ranges(display0.range, make_range(0, 1)));
    assert!(!display0.has_script);
    assert_eq!(display0.index(), 0);
    assert_eq!(display0.sub_displays().len(), 1);

    let sub0sub0 = &display0.sub_displays()[0];
    assert_eq!(sub0sub0.class_name(), "MTCTLineDisplay");
    let line1 = sub0sub0;
    assert_eq!(ct(line1).atoms.len(), 1);
    assert_eq!(string_of(line1), "1");
    assert!(point_equal(line1.position(), zero()));
    assert!(!line1.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTMathListDisplay");
    let display1 = sub1;
    assert_eq!(display1.line_position(), LinePosition::Subscript);
    // Due to italic correction, positioned before subscript.
    assert!(
        point_equal(display1.position(), point(8.16, -20.02)),
        "Got {:?}",
        display1.position()
    );
    assert!(equal_ranges(display1.range, make_range(0, 1)));
    assert!(!display1.has_script);
    assert_eq!(display1.index(), 0);
    assert_eq!(display1.sub_displays().len(), 1);

    let sub1sub0 = &display1.sub_displays()[0];
    assert_eq!(sub1sub0.class_name(), "MTCTLineDisplay");
    let line3 = sub1sub0;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "0");
    assert!(point_equal(line3.position(), zero()));
    assert!(!line3.has_script);

    let sub2 = &display.sub_displays()[2];
    assert_eq!(sub2.class_name(), "MTGlyphDisplay");
    let glyph = sub2;
    assert!(point_equal(glyph.position(), zero()));
    assert!(equal_ranges(glyph.range, make_range(0, 1)));
    assert!(glyph.has_script); // There are subscripts and superscripts

    let sub3 = &display.sub_displays()[3];
    assert_eq!(sub3.class_name(), "MTCTLineDisplay");
    let line2 = sub3;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑥");
    assert!(
        is_equal(point(31.433, 0.0), line2.position(), 0.01),
        "Got {:?}",
        line2.position()
    );
    assert!(
        equal_ranges(line2.range, make_range(1, 1)),
        "Got {:?} instead",
        line2.range
    );
    // The Swift test checks line1 here, not line2.
    assert!(!line1.has_script);

    assert_accuracy!(display.ascent(), 33.044, 0.001);
    assert_accuracy!(display.descent(), 20.328, 0.001);
    assert_accuracy!(display.width(), 42.873, 0.001);
}

#[test]
fn test_large_op_with_limits_text_with_scripts() {
    let font = default_font();
    let math_list = MTMathList::new();
    let op = MTMathAtomFactory::atom_for_latex_symbol("lim").unwrap();
    op.borrow_mut().set_sub_script(Some(MTMathList::new()));
    op.borrow()
        .sub_script()
        .unwrap()
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_latex_symbol("infty"));
    math_list.borrow_mut().add(Some(op));
    math_list
        .borrow_mut()
        .add(Some(MTMathAtom::with_type(MTMathAtomType::Variable, "x")));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 2)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTLargeOpLimitsDisplay");
    let large_op = sub0.as_large_op_limits().unwrap();
    assert!(point_equal(sub0.position(), zero()));
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(large_op.lower_limit.is_some());
    assert!(large_op.upper_limit.is_none());

    let display2 = large_op.lower_limit.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        is_equal(point(6.89, -12.00), display2.position(), 0.01),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let sub0sub0 = &display2.sub_displays()[0];
    assert_eq!(sub0sub0.class_name(), "MTCTLineDisplay");
    let line1 = sub0sub0;
    assert_eq!(ct(line1).atoms.len(), 1);
    assert_eq!(string_of(line1), "∞");
    assert!(point_equal(line1.position(), zero()));
    assert!(!line1.has_script);

    let sub3 = &display.sub_displays()[1];
    assert_eq!(sub3.class_name(), "MTCTLineDisplay");
    let line2 = sub3;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑥");
    assert!(
        is_equal(point(31.1133, 0.0), line2.position(), 0.01),
        "Got {:?}",
        line2.position()
    );
    assert!(
        equal_ranges(line2.range, make_range(1, 1)),
        "Got {:?} instead",
        line2.range
    );
    // The Swift test checks line1 here, not line2.
    assert!(!line1.has_script);

    assert_accuracy!(display.ascent(), 13.88, 0.01);
    assert_accuracy!(display.descent(), 12.154, 0.01);
    assert_accuracy!(display.width(), 42.553, 0.01);
}

#[test]
fn test_large_op_with_limits_symbolt_with_scripts() {
    let font = default_font();
    let math_list = MTMathList::new();
    let op = MTMathAtomFactory::atom_for_latex_symbol("sum").unwrap();
    op.borrow_mut().set_super_script(Some(MTMathList::new()));
    op.borrow()
        .super_script()
        .unwrap()
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_latex_symbol("infty"));
    op.borrow_mut().set_sub_script(Some(MTMathList::new()));
    op.borrow()
        .sub_script()
        .unwrap()
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("0"));
    math_list.borrow_mut().add(Some(op));
    math_list
        .borrow_mut()
        .add(Some(MTMathAtom::with_type(MTMathAtomType::Variable, "x")));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 2)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTLargeOpLimitsDisplay");
    let large_op = sub0.as_large_op_limits().unwrap();
    assert!(point_equal(sub0.position(), zero()));
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(large_op.lower_limit.is_some());
    assert!(large_op.upper_limit.is_some());

    let display2 = large_op.lower_limit.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        is_equal(point(10.94, -21.664), display2.position(), 0.01),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let sub0sub0 = &display2.sub_displays()[0];
    assert_eq!(sub0sub0.class_name(), "MTCTLineDisplay");
    let line1 = sub0sub0;
    assert_eq!(ct(line1).atoms.len(), 1);
    assert_eq!(string_of(line1), "0");
    assert!(point_equal(line1.position(), zero()));
    assert!(!line1.has_script);

    let display_u = large_op.upper_limit.as_deref().unwrap();
    assert_eq!(display_u.line_position(), LinePosition::Regular);
    assert!(
        is_equal(point(7.44, 23.154), display_u.position(), 0.01),
        "Got {:?}",
        display_u.position()
    );
    assert!(equal_ranges(display_u.range, make_range(0, 1)));
    assert!(!display_u.has_script);
    assert_eq!(display_u.index(), NSNotFound);
    assert_eq!(display_u.sub_displays().len(), 1);

    let sub0sub_u = &display_u.sub_displays()[0];
    assert_eq!(sub0sub_u.class_name(), "MTCTLineDisplay");
    let line3 = sub0sub_u;
    assert_eq!(ct(line3).atoms.len(), 1);
    assert_eq!(string_of(line3), "∞");
    assert!(point_equal(line3.position(), zero()));
    assert!(!line3.has_script);

    let sub3 = &display.sub_displays()[1];
    assert_eq!(sub3.class_name(), "MTCTLineDisplay");
    let line2 = sub3;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑥");
    assert!(
        is_equal(point(32.2133, 0.0), line2.position(), 0.01),
        "Got {:?}",
        line2.position()
    );
    assert!(
        equal_ranges(line2.range, make_range(1, 1)),
        "Got {:?} instead",
        line2.range
    );
    assert!(!line2.has_script);

    assert_accuracy!(display.ascent(), 29.342, 0.001);
    assert_accuracy!(display.descent(), 21.972, 0.001);
    assert_accuracy!(display.width(), 43.653, 0.001);
}

#[test]
fn test_inner() {
    let font = default_font();
    let inner_list = MTMathList::new();
    inner_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));
    let inner = MTMathAtom::inner();
    inner.borrow_mut().set_inner_list(Some(inner_list));
    inner
        .borrow_mut()
        .as_inner_mut()
        .unwrap()
        .set_left_boundary(Some(MTMathAtom::with_type(MTMathAtomType::Boundary, "(")));
    inner
        .borrow_mut()
        .as_inner_mut()
        .unwrap()
        .set_right_boundary(Some(MTMathAtom::with_type(MTMathAtomType::Boundary, ")")));

    let math_list = MTMathList::new();
    math_list.borrow_mut().add(Some(inner));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTMathListDisplay");
    let display2 = sub0;
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(point_equal(display2.position(), zero()));
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 3);

    let sub_left = &display2.sub_displays()[0];
    assert_eq!(sub_left.class_name(), "MTGlyphDisplay");
    let glyph = sub_left;
    assert!(point_equal(glyph.position(), zero()));
    assert!(equal_ranges(glyph.range, make_range(NS_NOT_FOUND, 0)));
    assert!(!glyph.has_script);

    let sub3 = &display2.sub_displays()[1];
    assert_eq!(sub3.class_name(), "MTMathListDisplay");
    let display3 = sub3;
    assert_eq!(display3.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display3.position(), point(7.78, 0.0)),
        "Got {:?}",
        display3.position()
    );
    assert!(equal_ranges(display3.range, make_range(0, 1)));
    assert!(!display3.has_script);
    assert_eq!(display3.index(), NSNotFound);
    assert_eq!(display3.sub_displays().len(), 1);

    let subsub3 = &display3.sub_displays()[0];
    assert_eq!(subsub3.class_name(), "MTCTLineDisplay");
    let line = subsub3;
    assert_eq!(ct(line).atoms.len(), 1);
    // The x is italicized
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(!line.has_script);

    let sub_right = &display2.sub_displays()[2];
    assert_eq!(sub_right.class_name(), "MTGlyphDisplay");
    let glyph2 = sub_right;
    assert!(
        point_equal(glyph2.position(), point(19.22, 0.0)),
        "Got {:?}",
        glyph2.position()
    );
    assert!(
        equal_ranges(glyph2.range, make_range(NS_NOT_FOUND, 0)),
        "Got {:?} instead",
        glyph2.range
    );
    assert!(!glyph2.has_script);

    // dimensions
    assert_eq!(display.ascent(), display2.ascent());
    assert_eq!(display.descent(), display2.descent());
    assert_eq!(display.width(), display2.width());

    assert_accuracy!(display.ascent(), 14.96, 0.001);
    assert_accuracy!(display.descent(), 4.96, 0.001);
    assert_accuracy!(display.width(), 27.0, 0.01);
}

#[test]
fn test_overline() {
    let font = default_font();
    let math_list = MTMathList::new();
    let over = MTMathAtom::over_line();
    let inner = MTMathList::new();
    inner
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    over.borrow_mut().set_inner_list(Some(inner));
    math_list.borrow_mut().add(Some(over));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTLineDisplay");
    let overline = sub0.as_line().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(overline.inner.is_some());

    let display2 = overline.inner.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), zero()),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subover = &display2.sub_displays()[0];
    assert_eq!(subover.class_name(), "MTCTLineDisplay");
    let line2 = subover;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 17.32, 0.01);
    assert_accuracy!(display.descent(), 0.00, 0.01);
    assert_accuracy!(display.width(), 10.0, 0.01);
}

#[test]
fn test_underline() {
    let font = default_font();
    let math_list = MTMathList::new();
    let under = MTMathAtom::under_line();
    let inner = MTMathList::new();
    inner
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("1"));
    under.borrow_mut().set_inner_list(Some(inner));
    math_list.borrow_mut().add(Some(under));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTLineDisplay");
    let underline = sub0.as_line().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(underline.inner.is_some());

    let display2 = underline.inner.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), zero()),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subover = &display2.sub_displays()[0];
    assert_eq!(subover.class_name(), "MTCTLineDisplay");
    let line2 = subover;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "1");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 13.32, 0.01);
    assert_accuracy!(display.descent(), 4.00, 0.01);
    assert_accuracy!(display.width(), 10.0, 0.01);
}

#[test]
fn test_spacing() {
    let font = default_font();
    let math_list = MTMathList::new();
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));
    math_list.borrow_mut().add(Some(MTMathAtom::space(9.0)));
    math_list
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("y"));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(
        equal_ranges(display.range, make_range(0, 3)),
        "Got {:?} instead",
        display.range
    );
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 2);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    // The x is italicized
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 1)));
    assert!(!line.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTCTLineDisplay");
    let line2 = sub1;
    assert_eq!(ct(line2).atoms.len(), 1);
    // The y is italicized
    assert_eq!(string_of(line2), "𝑦");
    assert!(
        is_equal(point(21.44, 0.0), line2.position(), 0.01),
        "Got {:?}",
        line2.position()
    );
    assert!(
        equal_ranges(line2.range, make_range(2, 1)),
        "Got {:?} instead",
        line2.range
    );
    assert!(!line2.has_script);

    let no_space = MTMathList::new();
    no_space
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));
    no_space
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("y"));

    let no_space_display = typeset(Some(&no_space), &font, MTLineStyle::Display);

    // dimensions
    assert_accuracy!(display.ascent(), no_space_display.ascent(), 0.01);
    assert_accuracy!(display.descent(), no_space_display.descent(), 0.01);
    assert_accuracy!(display.width(), no_space_display.width() + 10.0, 0.01);
}

// For issue: https://github.com/kostub/iosMath/issues/5
#[test]
fn test_large_radical_descent() {
    let font = default_font();
    let list = MTMathListBuilder::build_from_string(
        "\\sqrt{\\frac{\\sqrt{\\frac{1}{2}} + 3}{\\sqrt{5}^x}}",
    );
    let display = typeset(list.as_ref(), &font, MTLineStyle::Display);

    // dimensions
    assert_accuracy!(display.ascent(), 49.16, 0.01);
    assert_accuracy!(display.descent(), 21.288, 0.01);
    assert_accuracy!(display.width(), 82.569, 0.01);
}

#[test]
fn test_math_table() {
    let font = default_font();
    let c00 = MTMathAtomFactory::math_list_for_characters("1");
    let c01 = MTMathAtomFactory::math_list_for_characters("y+z");
    let c02 = MTMathAtomFactory::math_list_for_characters("y");

    let c11 = MTMathList::new();
    c11.borrow_mut()
        .add(Some(MTMathAtomFactory::fraction_with_strings("1", "2x")));
    let c12 = MTMathAtomFactory::math_list_for_characters("x-y");

    let c20 = MTMathAtomFactory::math_list_for_characters("x+5");
    let c22 = MTMathAtomFactory::math_list_for_characters("12");

    let table = MTMathAtom::table(None);
    {
        let mut atom = table.borrow_mut();
        let t = atom.as_table_mut().unwrap();
        t.set_cell(c00.unwrap(), 0, 0);
        t.set_cell(c01.unwrap(), 0, 1);
        t.set_cell(c02.unwrap(), 0, 2);
        t.set_cell(c11, 1, 1);
        t.set_cell(c12.unwrap(), 1, 2);
        t.set_cell(c20.unwrap(), 2, 0);
        t.set_cell(c22.unwrap(), 2, 2);

        // alignments
        t.set_alignment(MTColumnAlignment::Right, 0);
        t.set_alignment(MTColumnAlignment::Left, 2);

        t.inter_column_spacing = 18.0; // 1 quad
    }

    let math_list = MTMathList::new();
    math_list.borrow_mut().add(Some(table));

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTMathListDisplay");

    let display2 = sub0;
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), zero()),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 3);
    let row_pos = [30.28, -2.68, -31.95];
    // alignment is right, center, left.
    let cell_pos = [
        [35.89, 65.89, 129.438],
        [45.89, 76.94, 129.438],
        [0.0, 87.66, 129.438],
    ];
    // check the 3 rows of the matrix
    for i in 0..3 {
        let sub0i = &display2.sub_displays()[i];
        assert_eq!(sub0i.class_name(), "MTMathListDisplay");

        let row = sub0i;
        assert_eq!(row.line_position(), LinePosition::Regular);
        assert!(
            is_equal(point(0.0, row_pos[i]), row.position(), 0.01),
            "row {i}: got {:?}",
            row.position()
        );
        assert!(equal_ranges(row.range, make_range(0, 3)));
        assert!(!row.has_script);
        assert_eq!(row.index(), NSNotFound);
        assert_eq!(row.sub_displays().len(), 3);

        for j in 0..3 {
            let sub0ij = &row.sub_displays()[j];
            assert_eq!(sub0ij.class_name(), "MTMathListDisplay");

            let col = sub0ij;
            assert_eq!(col.line_position(), LinePosition::Regular);
            assert!(
                is_equal(point(cell_pos[i][j], 0.0), col.position(), 0.01),
                "cell ({i}, {j}): got {:?}",
                col.position()
            );
            assert!(!col.has_script);
            assert_eq!(col.index(), NSNotFound);
        }
    }
}

#[test]
fn test_latex_symbols() {
    let font = default_font();
    // Test all latex symbols
    let all_symbols = MTMathAtomFactory::supported_latex_symbol_names();
    for sym_name in all_symbols {
        let list = MTMathList::new();
        let atom = MTMathAtomFactory::atom_for_latex_symbol(&sym_name);
        assert!(atom.is_some());
        let atom = atom.unwrap();
        if atom.borrow().type_ >= MTMathAtomType::Boundary {
            // Skip these types as they aren't symbols.
            continue;
        }

        list.borrow_mut().add(Some(atom.clone()));

        let display = typeset(Some(&list), &font, MTLineStyle::Display);
        // XCTAssertNotNil(display, "Symbol \(symName)"): the unwrap in typeset.

        assert_eq!(display.line_position(), LinePosition::Regular);
        assert!(point_equal(display.position(), zero()));
        assert!(equal_ranges(display.range, make_range(0, 1)));
        assert!(!display.has_script);
        assert_eq!(display.index(), NSNotFound);
        assert_eq!(display.sub_displays().len(), 1, "Symbol {sym_name}");

        let sub0 = &display.sub_displays()[0];
        let (atom_type, nucleus) = {
            let atom = atom.borrow();
            (atom.type_, atom.nucleus.clone())
        };
        if atom_type == MTMathAtomType::LargeOperator && swift::count(&nucleus) == 1 {
            // These large operators are rendered differently;
            assert_eq!(sub0.class_name(), "MTGlyphDisplay");
            let glyph = sub0;
            assert!(point_equal(glyph.position(), zero()));
            assert!(equal_ranges(glyph.range, make_range(0, 1)));
            assert!(!glyph.has_script);
        } else {
            assert_eq!(sub0.class_name(), "MTCTLineDisplay", "Symbol {sym_name}");
            let line = sub0;
            assert_eq!(ct(line).atoms.len(), 1);
            if atom_type != MTMathAtomType::Variable {
                let string = string_of(line);
                assert!(
                    swift::equal(&string, &nucleus),
                    "Symbol {sym_name}: {string:?} != {nucleus:?}"
                );
            }
            assert!(point_equal(line.position(), zero()));
            assert!(equal_ranges(line.range, make_range(0, 1)));
            assert!(!line.has_script);
        }

        // dimensions
        assert_eq!(display.ascent(), sub0.ascent());
        assert_eq!(display.descent(), sub0.descent());
        assert_eq!(display.width(), sub0.width());

        // All chars will occupy some space.
        if nucleus != " " {
            // all chars except space have height
            assert!(
                display.ascent() + display.descent() > 0.0,
                "Symbol {sym_name}"
            );
        }
        // all chars have a width.
        assert!(display.width() > 0.0);
    }
}

/// `testAtomWithAllFontStyles(_:)`: a helper, not a test (it takes an argument).
fn test_atom_with_all_font_styles(font: &Arc<MTFont>, atom: Option<&MTMathAtomRef>) {
    let Some(atom) = atom else { return };
    let font_styles = [
        MTFontStyle::DefaultStyle,
        MTFontStyle::Roman,
        MTFontStyle::Bold,
        MTFontStyle::Caligraphic,
        MTFontStyle::Typewriter,
        MTFontStyle::Italic,
        MTFontStyle::SansSerif,
        MTFontStyle::Fraktur,
        MTFontStyle::Blackboard,
        MTFontStyle::BoldItalic,
    ];
    let nucleus = atom.borrow().nucleus.clone();
    for font_style in font_styles {
        let style = font_style;
        let copy = atom.borrow().copy();
        copy.borrow_mut().font_style = style;
        let list = MTMathList::with_atom(copy);

        let display = typeset(Some(&list), font, MTLineStyle::Display);
        // XCTAssertNotNil(display, "Symbol \(atom.nucleus)"): the unwrap in typeset.

        assert_eq!(display.line_position(), LinePosition::Regular);
        assert!(point_equal(display.position(), zero()));
        assert!(equal_ranges(display.range, make_range(0, 1)));
        assert!(!display.has_script);
        assert_eq!(display.index(), NSNotFound);
        assert_eq!(display.sub_displays().len(), 1, "Symbol {nucleus}");

        let sub0 = &display.sub_displays()[0];
        assert_eq!(sub0.class_name(), "MTCTLineDisplay", "Symbol {nucleus}");
        let line = sub0;
        assert_eq!(ct(line).atoms.len(), 1);
        assert!(point_equal(line.position(), zero()));
        assert!(equal_ranges(line.range, make_range(0, 1)));
        assert!(!line.has_script);

        // dimensions
        assert_eq!(display.ascent(), sub0.ascent());
        assert_eq!(display.descent(), sub0.descent());
        assert_eq!(display.width(), sub0.width());

        // All chars will occupy some space.
        assert!(
            display.ascent() + display.descent() > 0.0,
            "Symbol {nucleus} ({style:?})"
        );
        // all chars have a width.
        assert!(display.width() > 0.0);
    }
}

#[test]
fn test_variables() {
    let font = default_font();
    // Test all variables
    let all_symbols = MTMathAtomFactory::supported_latex_symbol_names();
    for sym_name in all_symbols {
        let atom = MTMathAtomFactory::atom_for_latex_symbol(&sym_name).unwrap();
        // XCTAssertNotNil(atom): the unwrap above.
        if atom.borrow().type_ != MTMathAtomType::Variable {
            // Skip these types as we are only interested in variables.
            continue;
        }
        test_atom_with_all_font_styles(&font, Some(&atom));
    }
    let alpha_num = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.";
    let math_list = MTMathAtomFactory::math_list_for_characters(alpha_num);
    let atoms = math_list.unwrap().borrow().atoms.clone();
    for atom in &atoms {
        test_atom_with_all_font_styles(&font, Some(atom));
    }
}

#[test]
fn test_style_changes() {
    let font = default_font();
    let frac = MTMathAtomFactory::fraction_with_strings("1", "2");
    let list = MTMathList::with_atoms(vec![frac.clone()]);
    let style = MTMathAtom::style(MTLineStyle::Text);
    let text_list = MTMathList::with_atoms(vec![style, frac]);

    // This should make the display same as text.
    let display = typeset(Some(&text_list), &font, MTLineStyle::Display);
    let text_display = typeset(Some(&list), &font, MTLineStyle::Text);
    let original_display = typeset(Some(&list), &font, MTLineStyle::Display);

    // Display should be the same as rendering the fraction in text style.
    assert_eq!(display.ascent(), text_display.ascent());
    assert_eq!(display.descent(), text_display.descent());
    assert_eq!(display.width(), text_display.width());

    // Original display should be larger than display since it is greater.
    assert!(original_display.ascent() > display.ascent());
    assert!(original_display.descent() > display.descent());
    assert!(original_display.width() > display.width());
}

#[test]
fn test_style_middle() {
    let font = default_font();
    let atom1 = MTMathAtomFactory::atom_for_character("x").unwrap();
    let style1 = MTMathAtom::style(MTLineStyle::Script);
    let atom2 = MTMathAtomFactory::atom_for_character("y").unwrap();
    let style2 = MTMathAtom::style(MTLineStyle::ScriptOfScript);
    let atom3 = MTMathAtomFactory::atom_for_character("z").unwrap();
    let list = MTMathList::with_atoms(vec![atom1, style1, atom2, style2, atom3]);

    let display = typeset(Some(&list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 5)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 3);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTCTLineDisplay");
    let line = sub0;
    assert_eq!(ct(line).atoms.len(), 1);
    assert_eq!(string_of(line), "𝑥");
    assert!(point_equal(line.position(), zero()));
    assert!(equal_ranges(line.range, make_range(0, 1)));
    assert!(!line.has_script);

    let sub1 = &display.sub_displays()[1];
    assert_eq!(sub1.class_name(), "MTCTLineDisplay");
    let line1 = sub1;
    assert_eq!(ct(line1).atoms.len(), 1);
    assert_eq!(string_of(line1), "𝑦");
    assert!(equal_ranges(line1.range, make_range(2, 1)));
    assert!(!line1.has_script);

    let sub2 = &display.sub_displays()[2];
    assert_eq!(sub2.class_name(), "MTCTLineDisplay");
    let line2 = sub2;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑧");
    assert!(equal_ranges(line2.range, make_range(4, 1)));
    assert!(!line2.has_script);
}

#[test]
fn test_accent() {
    let font = default_font();
    let math_list = MTMathList::new();
    let accent = MTMathAtomFactory::accent_with_name("hat");
    let inner = MTMathList::new();
    inner
        .borrow_mut()
        .add(MTMathAtomFactory::atom_for_character("x"));
    if let Some(accent) = &accent {
        accent.borrow_mut().set_inner_list(Some(inner));
    }
    math_list.borrow_mut().add(accent);

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTAccentDisplay");
    let accent_disp = sub0.as_accent().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(accent_disp.accentee.is_some());
    assert!(accent_disp.accent.is_some());

    let display2 = accent_disp.accentee.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), zero()),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 1)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subaccentee = &display2.sub_displays()[0];
    assert_eq!(subaccentee.class_name(), "MTCTLineDisplay");
    let line2 = subaccentee;
    assert_eq!(ct(line2).atoms.len(), 1);
    assert_eq!(string_of(line2), "𝑥");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 1)));
    assert!(!line2.has_script);

    let glyph = accent_disp.accent.as_deref().unwrap();
    assert!(
        point_equal(glyph.position(), point(11.86, 0.0)),
        "Got {:?}",
        glyph.position()
    );
    assert!(equal_ranges(glyph.range, make_range(0, 1)));
    assert!(!glyph.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 14.68, 0.01);
    assert_accuracy!(display.descent(), 0.22, 0.01);
    assert_accuracy!(display.width(), 11.44, 0.01);
}

#[test]
fn test_wide_accent() {
    let font = default_font();
    let math_list = MTMathList::new();
    let accent = MTMathAtomFactory::accent_with_name("hat");
    if let Some(accent) = &accent {
        accent
            .borrow_mut()
            .set_inner_list(MTMathAtomFactory::math_list_for_characters("xyzw"));
    }
    math_list.borrow_mut().add(accent);

    let display = typeset(Some(&math_list), &font, MTLineStyle::Display);
    assert_eq!(display.line_position(), LinePosition::Regular);
    assert!(point_equal(display.position(), zero()));
    assert!(equal_ranges(display.range, make_range(0, 1)));
    assert!(!display.has_script);
    assert_eq!(display.index(), NSNotFound);
    assert_eq!(display.sub_displays().len(), 1);

    let sub0 = &display.sub_displays()[0];
    assert_eq!(sub0.class_name(), "MTAccentDisplay");
    let accent_disp = sub0.as_accent().unwrap();
    assert!(equal_ranges(sub0.range, make_range(0, 1)));
    assert!(!sub0.has_script);
    assert!(point_equal(sub0.position(), zero()));
    assert!(accent_disp.accentee.is_some());
    assert!(accent_disp.accent.is_some());

    let display2 = accent_disp.accentee.as_deref().unwrap();
    assert_eq!(display2.line_position(), LinePosition::Regular);
    assert!(
        point_equal(display2.position(), zero()),
        "Got {:?}",
        display2.position()
    );
    assert!(equal_ranges(display2.range, make_range(0, 4)));
    assert!(!display2.has_script);
    assert_eq!(display2.index(), NSNotFound);
    assert_eq!(display2.sub_displays().len(), 1);

    let subaccentee = &display2.sub_displays()[0];
    assert_eq!(subaccentee.class_name(), "MTCTLineDisplay");
    let line2 = subaccentee;
    assert_eq!(ct(line2).atoms.len(), 4);
    assert_eq!(string_of(line2), "𝑥𝑦𝑧𝑤");
    assert!(point_equal(line2.position(), zero()));
    assert!(equal_ranges(line2.range, make_range(0, 4)));
    assert!(!line2.has_script);

    let glyph = accent_disp.accent.as_deref().unwrap();
    assert!(
        is_equal(point(3.47, 0.0), glyph.position(), 0.01),
        "Got {:?}",
        glyph.position()
    );
    assert!(equal_ranges(glyph.range, make_range(0, 1)));
    assert!(!glyph.has_script);

    // dimensions
    assert_accuracy!(display.ascent(), 14.98, 0.01);
    assert_accuracy!(display.descent(), 4.10, 0.01);
    assert_accuracy!(display.width(), 44.86, 0.01);
}
