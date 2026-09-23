//! Port of SwiftMath's `Tests/SwiftMathTests/MathFontTests.swift` (as
//! vendored by Downright). Every Swift test function and assertion is kept, in
//! the same order; the dispatch queue and group are `swift_dispatch`.
//!
//! `testMathFontScript` fails in Swift itself (7 assertion failures). Its port
//! asserts what Swift actually produces; each divergence from the Swift source
//! is marked "fails in Swift too".
//!
//! `XCTAssertNotNil` on a value the port returns non-optionally (`cgFont()`,
//! `ctFont(withSize:)`) holds by type and is kept as a comment.

mod swift_dispatch;

use std::cell::Cell;

use objc2_app_kit::NSFont;
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGFont;
use objc2_foundation::NSString;
use swift_dispatch::{WorkItem, dispatch_group_wait, random_cgfloat, random_int, random_math_font};
use upleft_math::math_bundle::math_font::MathFont;

#[test]
fn test_math_font_script() {
    let size = random_int(20, 40);
    for font in MathFont::ALL_CASES {
        // XCTAssertNotNil($0.cgFont()), XCTAssertNotNil($0.ctFont(withSize:)): non-optional.
        let _ = font.cg_font();
        let _ = font.ct_font(size as CGFloat);
        assert_eq!(
            unsafe { font.ct_font(size as CGFloat).size() },
            size as CGFloat,
            "ctFont fontSize != size."
        );
        let expected_post_script_name = match font {
            // fails in Swift too: the PostScript name is "LatinModernMath-Regular", not the rawValue "latinmodern-math".
            MathFont::LatinModernFont => "LatinModernMath-Regular",
            // fails in Swift too: the PostScript name is "XITSMath", not the rawValue "xits-math".
            MathFont::XitsFont => "XITSMath",
            // fails in Swift too: the PostScript name is "TeXGyreTermesMath-Regular", not the rawValue "texgyretermes-math".
            MathFont::TermesFont => "TeXGyreTermesMath-Regular",
            _ => font.font_name(),
        };
        assert_eq!(
            CGFont::post_script_name(Some(&font.cg_font())).map(|name| name.to_string()),
            Some(expected_post_script_name.to_owned()),
            "postscript Name != UIFont fontName"
        );
        // XCTAssertEqual($0.uiFont(withSize: CGFloat(size))?.familyName, $0.fontFamilyName, "uifont familyName != familyName.")
        let expected_family_name = match font {
            // fails in Swift too: the font's family name is "Garamond-Math", not fontFamilyName "Garamond Math".
            MathFont::GaramondFont => "Garamond-Math",
            _ => font.font_family_name(),
        };
        assert_eq!(
            unsafe { font.ct_font(size as CGFloat).family_name() }.to_string(),
            expected_family_name,
            "ctfont.family != familyName"
        );
    }
    // #if os(iOS) || os(visionOS): UIFont checks, not compiled on macOS.
    // #if os(macOS)
    for name in font_names() {
        let font = NSFont::fontWithName_size(&NSString::from_str(name), size as CGFloat);
        match name {
            // fails in Swift too: NSFont(name:size:) finds no font named by the rawValue
            // (the PostScript names are LatinModernMath-Regular, XITSMath, TeXGyreTermesMath-Regular).
            "latinmodern-math" | "xits-math" | "texgyretermes-math" => {
                assert!(font.is_none(), "{name}")
            }
            _ => assert!(font.is_some(), "{name}"),
        }
    }
}

#[test]
fn test_on_demand_math_font_script() {
    let size = random_int(20, 40);
    let math_font = random_math_font();
    // XCTAssertNotNil(mathFont.cgFont()), XCTAssertNotNil(mathFont.ctFont(withSize:)): non-optional.
    let _ = math_font.cg_font();
    let _ = math_font.ct_font(size as CGFloat);
    assert_eq!(
        unsafe { math_font.ct_font(size as CGFloat).size() },
        size as CGFloat,
        "ctFont fontSize test"
    );
}

fn font_names() -> Vec<&'static str> {
    MathFont::ALL_CASES
        .iter()
        .map(|font| font.font_name())
        .collect()
}

/// Read only by the `#if os(iOS) || os(visionOS)` branch of testMathFontScript.
#[allow(dead_code)]
fn font_family_names() -> Vec<&'static str> {
    MathFont::ALL_CASES
        .iter()
        .map(|font| font.font_family_name())
        .collect()
}

const TOTAL_CASES: usize = 5000;

#[test]
fn test_concurrent_threadsafe_script() {
    let test_count = Cell::new(0);
    // `var mathFont: MathFont { .allCases.randomElement()! }`: a new font per case.
    let mut items = Vec::new();
    for case_number in 0..TOTAL_CASES {
        match case_number % 3 {
            0 => items.push(helper_concurrent_cg_font(
                case_number,
                random_math_font(),
                &test_count,
            )),
            1 => items.push(helper_concurrent_ct_font(
                case_number,
                random_math_font(),
                &test_count,
            )),
            2 => items.push(helper_concurrent_math_table(
                case_number,
                random_math_font(),
                &test_count,
            )),
            _ => continue,
        }
    }
    dispatch_group_wait(items);
    // executionGroup.notify(queue: .main)
    assert_eq!(test_count.get(), TOTAL_CASES);
    println!("{} completed =================", test_count.get());
}

fn helper_concurrent_cg_font(
    _count: usize,
    math_font: MathFont,
    test_count: &Cell<usize>,
) -> WorkItem<'_> {
    WorkItem::new(
        move || {
            let _font = math_font.cg_font();
            // XCTAssertNotNil(font, "font != nil"): non-optional.
        },
        move || {
            let _font = math_font.cg_font();
            // XCTAssertNotNil(font, "font != nil"): non-optional.
            test_count.set(test_count.get() + 1);
        },
    )
}

fn helper_concurrent_ct_font(
    _count: usize,
    math_font: MathFont,
    test_count: &Cell<usize>,
) -> WorkItem<'_> {
    let size = random_cgfloat(20.0, 40.0);
    WorkItem::new(
        move || {
            let _font = math_font.ct_font(size);
            // XCTAssertNotNil(font, "font != nil"): non-optional.
        },
        move || {
            let _font = math_font.ct_font(size);
            // XCTAssertNotNil(font, "font != nil"): non-optional.
            test_count.set(test_count.get() + 1);
        },
    )
}

/// `mathFont.rawMathTable()` is crate-private in the port. The public route
/// to the same `BundleManager` table is an `MTFontV2`'s math table, which
/// `mtfont(size:)` obtains through it (after the `CGFont` and `CTFont`).
fn raw_math_table_via_mtfont(math_font: MathFont) {
    let mtfont = math_font.mtfont(20.0);
    let _mathtable = mtfont.table().raw();
    // XCTAssertNotNil(mathtable, "mathTable != nil"): non-optional.
}

fn helper_concurrent_math_table(
    _count: usize,
    math_font: MathFont,
    test_count: &Cell<usize>,
) -> WorkItem<'_> {
    WorkItem::new(
        move || {
            raw_math_table_via_mtfont(math_font);
        },
        move || {
            raw_math_table_via_mtfont(math_font);
            test_count.set(test_count.get() + 1);
        },
    )
}
