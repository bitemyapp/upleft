//! Port of SwiftMath's `Tests/SwiftMathTests/MTFontMathTableV2Tests.swift`
//! (as vendored by Downright). Every Swift test function and assertion is
//! kept, in the same order; the dispatch queue and group are `swift_dispatch`.

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

/// `[mTable?.fractionNumeratorDisplayStyleShiftUp, …].compactMap{$0}`.
fn fraction_values(m_table: Option<&MTFontMathTableV2>) -> Vec<CGFloat> {
    [
        m_table.map(|t| t.fraction_numerator_display_style_shift_up()),
        m_table.map(|t| t.fraction_numerator_shift_up()),
        m_table.map(|t| t.fraction_denominator_display_style_shift_down()),
        m_table.map(|t| t.fraction_denominator_shift_down()),
        m_table.map(|t| t.fraction_numerator_display_style_gap_min()),
        m_table.map(|t| t.fraction_numerator_gap_min()),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn test_mt_font_v2_script() {
    let size = random_int(20, 40) as CGFloat;
    for font in MathFont::ALL_CASES {
        let mtfont = font.mtfont(size);
        let m_table = mtfont.math_table();
        assert!(m_table.is_some());
        let values = fraction_values(m_table);
        println!("{}.plist: {:?}", font.raw_value(), values);
    }
}

#[test]
fn test_concurrent_threadsafe_script() {
    let test_count = Cell::new(0);
    // `var mathFont` and `var size` are computed: a new font and size per element.
    let mtfonts: Vec<MTFontV2> = (0..10)
        .map(|_| random_math_font().mtfont(random_cgfloat(20.0, 40.0)))
        .collect();
    let indices: Vec<usize> = (0..mtfonts.len()).collect();
    let items = (0..TOTAL_CASES)
        .map(|case_number| {
            let mtfont = &mtfonts[random_element(&indices)];
            helper_concurrent_mt_font_math_table_v2(case_number, mtfont, &test_count)
        })
        .collect();
    dispatch_group_wait(items);
    // executionGroup.notify(queue: .main)
    assert_eq!(test_count.get(), TOTAL_CASES);
    println!("{} completed =================", test_count.get());
}

fn helper_concurrent_mt_font_math_table_v2<'a>(
    count: usize,
    mtfont: &'a MTFontV2,
    test_count: &'a Cell<usize>,
) -> WorkItem<'a> {
    WorkItem::new(
        move || {
            let m_table = mtfont.math_table();
            let _values = fraction_values(m_table);
            // if count % 50 == 0 {
            //     print(values) // accessed these values on global thread.
            // }
            assert!(m_table.is_some());
        },
        move || {
            let m_table = mtfont.math_table();
            if count.is_multiple_of(70) {
                let _values = fraction_values(m_table);
                // if count % 50 == 0 {
                //     print(values)
                // }
            }
            test_count.set(test_count.get() + 1);
        },
    )
}
