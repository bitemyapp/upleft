//! Port of SwiftMath's `Tests/SwiftMathTests/MTFontV2Tests.swift` (as
//! vendored by Downright). Every Swift test function and assertion is kept, in
//! the same order; the dispatch queue and group are `swift_dispatch`.
//!
//! `XCTAssertNotNil` on a value the port returns non-optionally (`mtfont`,
//! `defaultCGFont`, `ctFont`) holds by type and is kept as a comment.

mod swift_dispatch;

use std::cell::Cell;

use objc2_core_foundation::CGFloat;
use swift_dispatch::{
    WorkItem, dispatch_group_wait, random_cgfloat, random_element, random_int, random_math_font,
};
use upleft_math::math_bundle::math_font::MathFont;
use upleft_math::math_bundle::mt_font_math_table_v2::MTFontMathTableV2;
use upleft_math::math_bundle::mt_font_v2::MTFontV2;

const TOTAL_CASES: usize = 1000;

#[test]
fn test_mt_font_v2_script() {
    let size = random_int(20, 40) as CGFloat;
    for font in MathFont::ALL_CASES {
        let mtfont = font.mtfont(size);
        // `mtfont.mathTable?._mathTable`
        let m_table = mtfont.math_table().map(|table| table.raw());
        // XCTAssertNotNil(mtfont): non-optional.
        assert!(m_table.is_some());
    }
}

#[test]
fn test_concurrent_threadsafe_script() {
    let test_count = Cell::new(0);
    // `var mathFont: MathFont { .allCases.randomElement()! }`: a new font per case.
    let items = (0..TOTAL_CASES)
        .map(|case_number| {
            helper_concurrent_mt_font_v2(case_number, random_math_font(), &test_count)
        })
        .collect();
    dispatch_group_wait(items);
    // executionGroup.notify(queue: .main)
    assert_eq!(test_count.get(), TOTAL_CASES);
    println!("{} completed =================", test_count.get());
}

fn helper_concurrent_mt_font_v2(
    _count: usize,
    math_font: MathFont,
    test_count: &Cell<usize>,
) -> WorkItem<'_> {
    let size = random_cgfloat(20.0, 40.0);
    WorkItem::new(
        move || {
            let font_v2: MTFontV2 = math_font.mtfont(size);
            // XCTAssertNotNil(fontV2): non-optional.
            let (_cgfont, _ctfont) = (font_v2.default_cg_font(), font_v2.ct_font());
            // XCTAssertNotNil(cgfont), XCTAssertNotNil(ctfont): non-optional.
        },
        move || {
            let font_v2 = math_font.mtfont(size);
            // XCTAssertNotNil(fontV2): non-optional.
            let (_cgfont, _ctfont) = (font_v2.default_cg_font(), font_v2.ct_font());
            // XCTAssertNotNil(cgfont), XCTAssertNotNil(ctfont): non-optional.
            // `mathFont.rawMathTable()` is crate-private in the port; the
            // font holds the same `BundleManager` table, which `mtfont(size:)`
            // just obtained through it.
            let _m_table = font_v2.table().raw();
            // XCTAssertNotNil(mTable): non-optional.
            test_count.set(test_count.get() + 1);
        },
    )
}

#[test]
fn test_concurrent_threadsafe_math_table_lock_script() {
    let test_count = Cell::new(0);
    // `var mathFont` and `var size` are computed: a new font and size per element.
    let mtfonts: Vec<MTFontV2> = (0..5)
        .map(|_| random_math_font().mtfont(random_cgfloat(20.0, 40.0)))
        .collect();
    let indices: Vec<usize> = (0..mtfonts.len()).collect();
    let items = (0..TOTAL_CASES)
        .map(|case_number| {
            let mtfont = &mtfonts[random_element(&indices)];
            helper_concurrent_mt_font_v2_math_table_lock(case_number, mtfont, &test_count)
        })
        .collect();
    dispatch_group_wait(items);
    // executionGroup.notify(queue: .main)
    assert_eq!(test_count.get(), TOTAL_CASES);
    println!("{} completed =================", test_count.get());
}

fn helper_concurrent_mt_font_v2_math_table_lock<'a>(
    _count: usize,
    mtfont: &'a MTFontV2,
    test_count: &'a Cell<usize>,
) -> WorkItem<'a> {
    WorkItem::new(
        move || {
            // `mtfont.mathTable as? MTFontMathTableV2`
            let math_table: Option<&MTFontMathTableV2> = mtfont.math_table();
            // each mathTable is initialized once per mtfont with a NSLock.
            // this is even when mathTable is accessed via different threads.
            assert!(math_table.is_some());
        },
        move || {
            test_count.set(test_count.get() + 1);
        },
    )
}
