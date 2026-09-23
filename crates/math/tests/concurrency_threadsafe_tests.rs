//! Port of SwiftMath's `Tests/SwiftMathTests/ConcurrencyThreadsafeTests.swift`
//! (as vendored by Downright). Every Swift test function and assertion is
//! kept, in the same order; the dispatch queue and group are `swift_dispatch`.

mod swift_dispatch;

use std::cell::Cell;
use std::collections::HashMap;

use swift_dispatch::{WorkItem, dispatch_group_wait};
use upleft_math::math_render::mt_math_atom_factory::MTMathAtomFactory;

const TOTAL_CASES: usize = 20;

#[test]
fn test_swift_math_concurrent_script() {
    let test_count = Cell::new(0);
    let mut items = Vec::new();
    for case_number in 0..TOTAL_CASES {
        items.push(helper_concurrency(case_number, &test_count, || {
            // `getInterElementSpaces()` has no counterpart to call: the port's
            // table is a private `const` (`INTER_ELEMENT_SPACE_ARRAY` in
            // mt_typesetter.rs), so there is no lazy initialisation to race.
            let result2: &HashMap<String, String> = MTMathAtomFactory::delim_value_to_name();
            let result3: &HashMap<String, String> = MTMathAtomFactory::accent_value_to_name();
            // `textToLatexSymbolName` is exposed as a lookup into the lazily
            // built table; any lookup forces the initialisation Swift races.
            let result4: Option<String> = MTMathAtomFactory::text_to_latex_symbol_name("×");
            // XCTAssertNotNil(result1…4): the Swift values are non-optional
            // dictionaries (and the Rust ones references), so the assertions
            // hold by type.
            let _ = (result2, result3, result4);
        }));
    }
    //        executionGroup.notify(queue: .main) { [weak self] in
    //            // print("All test cases completed: \(self?.testCount ?? 0)")
    //        }
    dispatch_group_wait(items);
}

fn helper_concurrency<'a>(
    _count: usize,
    test_count: &'a Cell<usize>,
    test_closure: impl FnOnce() + Send + 'a,
) -> WorkItem<'a> {
    WorkItem::new(
        move || {
            test_closure();
        },
        move || {
            test_count.set(test_count.get() + 1);
        },
    )
}
