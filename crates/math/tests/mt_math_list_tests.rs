//! Port of SwiftMath's `Tests/SwiftMathTests/MTMathListTests.swift` (as
//! vendored by Downright). Every Swift test function and assertion is kept, in
//! the same order.
//!
//! Atoms and lists are `NSObject`s in Swift without an `isEqual:` override, so
//! `XCTAssertEqual`/`XCTAssertNotEqual` on them compare identity: here
//! `Rc::ptr_eq`. An `NSException` inside `XCTExpectFailure(options: strict)
//! { XCTAssertThrowsError(…) }` is a panic here, caught with `catch_unwind`;
//! the panic message is checked against the exception reason the Swift run
//! reports.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use objc2_foundation::NSRange;
use upleft_math::math_render::mt_math_atom_factory::MTMathAtomFactory;
use upleft_math::math_render::mt_math_list::{
    MTColumnAlignment, MTLineStyle, MTMathAtom, MTMathAtomRef, MTMathAtomType, MTMathList,
    MTMathListRef,
};
use upleft_math::math_render::mt_math_list_builder::MTMathListBuilder;

/// `XCTExpectFailure(…, options: strict) { XCTAssertThrowsError(expression) }`:
/// the expression must raise, with the given reason.
fn assert_raises(expression: impl FnOnce(), reason: &str, test: &str) {
    let result = catch_unwind(AssertUnwindSafe(expression));
    let Err(payload) = result else {
        panic!("{test}: expected an exception ({reason}), none was raised");
    };
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert_eq!(message, reason, "{test}");
}

/// `list.atoms[i]`.
fn atom_at(list: &MTMathListRef, i: usize) -> MTMathAtomRef {
    list.borrow().atoms[i].clone()
}

/// `list.atoms.count`.
fn count(list: &MTMathListRef) -> usize {
    list.borrow().atoms.len()
}

/// `XCTAssertTrue(NSEqualRanges(atom.indexRange, NSMakeRange(location, length)))`.
fn assert_range(atom: &MTMathAtomRef, location: usize, length: usize, message: &str) {
    assert!(
        atom.borrow().index_range == NSRange::new(location, length),
        "{message}"
    );
}

#[test]
fn test_sub_script() {
    let str = "-52x^{13+y}_{15-} + (-12.3 *)\\frac{-12}{15.2}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let finalized = list.borrow().finalized();
    check_list_contents(&finalized);
    // refinalizing a finalized list should not cause any more changes
    check_list_contents(&finalized.borrow().finalized());
}

fn check_list_contents(finalized: &MTMathListRef) {
    // check
    assert_eq!(count(finalized), 10, "Num atoms");
    let mut atom = atom_at(finalized, 0);
    assert_eq!(atom.borrow().type_, MTMathAtomType::UnaryOperator, "Atom 0");
    assert_eq!(atom.borrow().nucleus, "−", "Atom 0 value");
    assert_range(&atom, 0, 1, "Range");
    atom = atom_at(finalized, 1);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Number, "Atom 1");
    assert_eq!(atom.borrow().nucleus, "52", "Atom 1 value");
    assert_range(&atom, 1, 2, "Range");
    atom = atom_at(finalized, 2);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Variable, "Atom 2");
    assert_eq!(atom.borrow().nucleus, "x", "Atom 2 value");
    assert_range(&atom, 3, 1, "Range");

    let super_scr = atom.borrow().super_script().cloned().unwrap();
    assert_eq!(count(&super_scr), 3, "Super script");
    atom = atom_at(&super_scr, 0);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Number, "Super Atom 0");
    assert_eq!(atom.borrow().nucleus, "13", "Super Atom 0 value");
    assert_range(&atom, 0, 2, "Range");
    atom = atom_at(&super_scr, 1);
    assert_eq!(
        atom.borrow().type_,
        MTMathAtomType::BinaryOperator,
        "Super Atom 1"
    );
    assert_eq!(atom.borrow().nucleus, "+", "Super Atom 1 value");
    assert_range(&atom, 2, 1, "Range");
    atom = atom_at(&super_scr, 2);
    assert_eq!(
        atom.borrow().type_,
        MTMathAtomType::Variable,
        "Super Atom 2"
    );
    assert_eq!(atom.borrow().nucleus, "y", "Super Atom 2 value");
    assert_range(&atom, 3, 1, "Range");

    atom = atom_at(finalized, 2);
    let sub_scr = atom.borrow().sub_script().cloned().unwrap();
    assert_eq!(count(&sub_scr), 2, "Sub script");
    atom = atom_at(&sub_scr, 0);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Number, "Sub Atom 0");
    assert_eq!(atom.borrow().nucleus, "15", "Sub Atom 0 value");
    assert_range(&atom, 0, 2, "Range");
    atom = atom_at(&sub_scr, 1);
    assert_eq!(
        atom.borrow().type_,
        MTMathAtomType::UnaryOperator,
        "Sub Atom 1"
    );
    assert_eq!(atom.borrow().nucleus, "−", "Sub Atom 1 value");
    assert_range(&atom, 2, 1, "Range");

    atom = atom_at(finalized, 3);
    assert_eq!(
        atom.borrow().type_,
        MTMathAtomType::BinaryOperator,
        "Atom 3"
    );
    assert_eq!(atom.borrow().nucleus, "+", "Atom 3 value");
    assert_range(&atom, 4, 1, "Range");
    atom = atom_at(finalized, 4);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Open, "Atom 4");
    assert_eq!(atom.borrow().nucleus, "(", "Atom 4 value");
    assert_range(&atom, 5, 1, "Range");
    atom = atom_at(finalized, 5);
    assert_eq!(atom.borrow().type_, MTMathAtomType::UnaryOperator, "Atom 5");
    assert_eq!(atom.borrow().nucleus, "−", "Atom 5 value");
    assert_range(&atom, 6, 1, "Range");
    atom = atom_at(finalized, 6);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Number, "Atom 6");
    assert_eq!(atom.borrow().nucleus, "12.3", "Atom 6 value");
    assert_range(&atom, 7, 4, "Range");
    atom = atom_at(finalized, 7);
    assert_eq!(atom.borrow().type_, MTMathAtomType::UnaryOperator, "Atom 7");
    assert_eq!(atom.borrow().nucleus, "*", "Atom 7 value");
    assert_range(&atom, 11, 1, "Range");
    atom = atom_at(finalized, 8);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Close, "Atom 8");
    assert_eq!(atom.borrow().nucleus, ")", "Atom 8 value");
    assert_range(&atom, 12, 1, "Range");

    // `finalized.atoms[9] as! MTFraction`
    let frac = atom_at(finalized, 9);
    assert!(frac.borrow().as_fraction().is_some(), "as! MTFraction");
    assert_eq!(frac.borrow().type_, MTMathAtomType::Fraction, "Atom 9");
    assert_eq!(frac.borrow().nucleus, "", "Atom 9 value");
    assert_range(&frac, 13, 1, "Range");

    let numer = frac
        .borrow()
        .as_fraction()
        .unwrap()
        .numerator
        .clone()
        .unwrap();
    assert_eq!(count(&numer), 2, "Numer script");
    atom = atom_at(&numer, 0);
    assert_eq!(
        atom.borrow().type_,
        MTMathAtomType::UnaryOperator,
        "Numer Atom 0"
    );
    assert_eq!(atom.borrow().nucleus, "−", "Numer Atom 0 value");
    assert_range(&atom, 0, 1, "Range");
    atom = atom_at(&numer, 1);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Number, "Numer Atom 1");
    assert_eq!(atom.borrow().nucleus, "12", "Numer Atom 1 value");
    assert_range(&atom, 1, 2, "Range");

    let denom = frac
        .borrow()
        .as_fraction()
        .unwrap()
        .denominator
        .clone()
        .unwrap();
    assert_eq!(count(&denom), 1, "Denom script");
    atom = atom_at(&denom, 0);
    assert_eq!(atom.borrow().type_, MTMathAtomType::Number, "Denom Atom 0");
    assert_eq!(atom.borrow().nucleus, "15.2", "Denom Atom 0 value");
    assert_range(&atom, 0, 4, "Range");
}

#[test]
fn test_add() {
    let list = MTMathList::new();
    assert_eq!(count(&list), 0);
    let atom = MTMathAtomFactory::placeholder();
    list.borrow_mut().add(Some(atom.clone()));
    assert_eq!(count(&list), 1);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom));
    let atom2 = MTMathAtomFactory::placeholder();
    list.borrow_mut().add(Some(atom2.clone()));
    assert_eq!(count(&list), 2);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom));
    assert!(Rc::ptr_eq(&atom_at(&list, 1), &atom2));
}

#[test]
fn test_add_errors() {
    let list = MTMathList::new();
    let mut atom: Option<MTMathAtomRef> = None;
    list.borrow_mut().add(atom);
    atom = Some(MTMathAtom::with_type(MTMathAtomType::Boundary, ""));
    // XCTExpectFailure("Test adding an illegal atom")
    assert_raises(
        || list.borrow_mut().add(atom),
        "Cannot add atom of type 101 into mathlist",
        "Test adding an illegal atom",
    );
}

#[test]
fn test_insert() {
    let list = MTMathList::new();
    assert_eq!(count(&list), 0);
    let atom = MTMathAtomFactory::placeholder();
    list.borrow_mut().insert(Some(atom.clone()), 0);
    assert_eq!(count(&list), 1);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom));
    let atom2 = MTMathAtomFactory::placeholder();
    list.borrow_mut().insert(Some(atom2.clone()), 0);
    assert_eq!(count(&list), 2);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom2));
    assert!(Rc::ptr_eq(&atom_at(&list, 1), &atom));
    let atom3 = MTMathAtomFactory::placeholder();
    list.borrow_mut().insert(Some(atom3.clone()), 2);
    assert_eq!(count(&list), 3);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom2));
    assert!(Rc::ptr_eq(&atom_at(&list, 1), &atom));
    assert!(Rc::ptr_eq(&atom_at(&list, 2), &atom3));
}

#[test]
fn test_insert_errors() {
    let list = MTMathList::new();
    let mut atom: Option<MTMathAtomRef> = None;
    list.borrow_mut().insert(atom, 0);
    atom = Some(MTMathAtom::with_type(MTMathAtomType::Boundary, ""));
    // XCTExpectFailure("Test adding an illegal atom")
    let illegal = atom.clone();
    assert_raises(
        || list.borrow_mut().insert(illegal, 0),
        "Cannot add atom of type 101 into mathlist",
        "Test adding an illegal atom",
    );
    atom = Some(MTMathAtomFactory::placeholder());
    list.borrow_mut().insert(atom, 1);
}

#[test]
fn test_append() {
    let list1 = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::placeholder();
    let atom3 = MTMathAtomFactory::placeholder();
    list1.borrow_mut().add(Some(atom));
    list1.borrow_mut().add(Some(atom2));
    list1.borrow_mut().add(Some(atom3));

    let list2 = MTMathList::new();
    let atom5 = MTMathAtomFactory::times();
    let atom6 = MTMathAtomFactory::divide();
    list2.borrow_mut().add(Some(atom5.clone()));
    list2.borrow_mut().add(Some(atom6.clone()));

    assert_eq!(count(&list1), 3);
    assert_eq!(count(&list2), 2);

    list1.borrow_mut().append(Some(&list2));
    assert_eq!(count(&list1), 5);
    assert!(Rc::ptr_eq(&atom_at(&list1, 3), &atom5));
    assert!(Rc::ptr_eq(&atom_at(&list1, 4), &atom6));
}

#[test]
fn test_remove_last() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    list.borrow_mut().add(Some(atom.clone()));
    assert_eq!(count(&list), 1);
    list.borrow_mut().remove_last_atom();
    assert_eq!(count(&list), 0);
    // Removing from empty list.
    list.borrow_mut().remove_last_atom();
    assert_eq!(count(&list), 0);
    let atom2 = MTMathAtomFactory::placeholder();
    list.borrow_mut().add(Some(atom.clone()));
    list.borrow_mut().add(Some(atom2));
    assert_eq!(count(&list), 2);
    list.borrow_mut().remove_last_atom();
    assert_eq!(count(&list), 1);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom));
}

#[test]
fn test_remove_atom_at_index() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::placeholder();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2.clone()));
    assert_eq!(count(&list), 2);
    list.borrow_mut().remove_atom(0);
    assert_eq!(count(&list), 1);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom2));

    // Index out of range
    // XCTExpectFailure("Test removing an out-of-index cell")
    assert_raises(
        || list.borrow_mut().remove_atom(2),
        "Index 2 out of bounds",
        "Test removing an out-of-index cell",
    );
}

#[test]
fn test_remove_atoms_in_range() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::placeholder();
    let atom3 = MTMathAtomFactory::placeholder();
    list.borrow_mut().add(Some(atom.clone()));
    list.borrow_mut().add(Some(atom2));
    list.borrow_mut().add(Some(atom3));
    assert_eq!(count(&list), 3);
    list.borrow_mut().remove_atoms(1..=2);
    assert_eq!(count(&list), 1);
    assert!(Rc::ptr_eq(&atom_at(&list, 0), &atom));

    // Index out of range
    // XCTExpectFailure("Test removing an out-of-bounds range")
    assert_raises(
        || list.borrow_mut().remove_atoms(1..=3),
        "Index 1 out of bounds",
        "Test removing an out-of-bounds range",
    );
}

fn check_atom_copy(copy: Option<&MTMathAtomRef>, original: Option<&MTMathAtomRef>, test: &str) {
    let (Some(copy), Some(original)) = (copy, original) else {
        return;
    };
    assert_eq!(copy.borrow().type_, original.borrow().type_, "{test}");
    assert_eq!(copy.borrow().nucleus, original.borrow().nucleus, "{test}");
    // Should be different objects with the same content
    assert!(!Rc::ptr_eq(copy, original), "{test}");
}

fn check_list_copy(copy: Option<&MTMathListRef>, original: Option<&MTMathListRef>, test: &str) {
    let (Some(copy), Some(original)) = (copy, original) else {
        return;
    };
    assert_eq!(count(copy), count(original), "{test}");
    for (i, copy_atom) in copy.borrow().atoms.iter().enumerate() {
        let orig_atom = atom_at(original, i);
        check_atom_copy(Some(copy_atom), Some(&orig_atom), test);
    }
}

/// `list.add(placeholder); list.add(times); list.add(divide)`, and a second
/// list of the divide and times atoms: the fixture most copy tests build.
fn fixture_lists() -> (MTMathListRef, MTMathListRef) {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::times();
    let atom3 = MTMathAtomFactory::divide();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2.clone()));
    list.borrow_mut().add(Some(atom3.clone()));

    let list2 = MTMathList::new();
    list2.borrow_mut().add(Some(atom3));
    list2.borrow_mut().add(Some(atom2));
    (list, list2)
}

#[test]
fn test_copy() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::times();
    let atom3 = MTMathAtomFactory::divide();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2));
    list.borrow_mut().add(Some(atom3));

    let list2 = MTMathList::copy_of(Some(&list));
    check_list_copy(list2.as_ref(), Some(&list), "test_copy");
}

#[test]
fn test_atom_init() {
    let mut atom = MTMathAtom::with_type(MTMathAtomType::Open, "(");
    assert_eq!(atom.borrow().nucleus, "(");
    assert_eq!(atom.borrow().type_, MTMathAtomType::Open);

    atom = MTMathAtom::with_type(MTMathAtomType::Radical, "(");
    assert_eq!(atom.borrow().nucleus, "");
    assert_eq!(atom.borrow().type_, MTMathAtomType::Radical);
}

#[test]
fn test_atom_scripts() {
    let mut atom = MTMathAtom::with_type(MTMathAtomType::Open, "(");
    assert!(atom.borrow().is_script_allowed());
    atom.borrow_mut().set_sub_script(Some(MTMathList::new()));
    assert!(atom.borrow().sub_script().is_some());
    atom.borrow_mut().set_super_script(Some(MTMathList::new()));
    assert!(atom.borrow().super_script().is_some());

    atom = MTMathAtom::with_type(MTMathAtomType::Boundary, "(");
    assert!(!atom.borrow().is_script_allowed());
    // Can set to nil
    atom.borrow_mut().set_sub_script(None);
    assert!(atom.borrow().sub_script().is_none());
    atom.borrow_mut().set_super_script(None);
    assert!(atom.borrow().super_script().is_none());
    // Can't set to value
    let list = MTMathList::new();

    // XCTExpectFailure("No sub/super-script on boundary atoms")
    assert_raises(
        || atom.borrow_mut().set_sub_script(Some(list.clone())),
        "Subscripts not allowed for atom of type Boundary",
        "No sub/super-script on boundary atoms",
    );
    assert_raises(
        || atom.borrow_mut().set_super_script(Some(list.clone())),
        "Superscripts not allowed for atom of type Boundary",
        "No sub/super-script on boundary atoms",
    );
}

#[test]
fn test_atom_copy() {
    let (list, list2) = fixture_lists();

    let atom = MTMathAtom::with_type(MTMathAtomType::Open, "(");
    atom.borrow_mut().set_sub_script(Some(list));
    atom.borrow_mut().set_super_script(Some(list2));
    let copy: MTMathAtomRef = atom.borrow().copy();

    let test = "test_atom_copy";
    check_atom_copy(Some(&copy), Some(&atom), test);
    check_list_copy(
        copy.borrow().super_script(),
        atom.borrow().super_script(),
        test,
    );
    check_list_copy(copy.borrow().sub_script(), atom.borrow().sub_script(), test);
}

#[test]
fn test_copy_fraction() {
    let (list, list2) = fixture_lists();

    let frac = MTMathAtom::fraction(false);
    assert_eq!(frac.borrow().type_, MTMathAtomType::Fraction);
    {
        let mut atom = frac.borrow_mut();
        let fraction = atom.as_fraction_mut().unwrap();
        fraction.numerator = Some(list);
        fraction.denominator = Some(list2);
        fraction.left_delimiter = "a".to_owned();
        fraction.right_delimiter = "b".to_owned();
    }

    // MTFraction(frac)
    let copy = frac.borrow().copy();
    let test = "test_copy_fraction";
    check_atom_copy(Some(&copy), Some(&frac), test);
    let copy_atom = copy.borrow();
    let copy_frac = copy_atom.as_fraction().unwrap();
    let frac_atom = frac.borrow();
    let orig_frac = frac_atom.as_fraction().unwrap();
    check_list_copy(
        copy_frac.numerator.as_ref(),
        orig_frac.numerator.as_ref(),
        test,
    );
    check_list_copy(
        copy_frac.denominator.as_ref(),
        orig_frac.denominator.as_ref(),
        test,
    );
    assert!(!copy_frac.has_rule);
    assert_eq!(copy_frac.left_delimiter, "a");
    assert_eq!(copy_frac.right_delimiter, "b");
}

#[test]
fn test_copy_radical() {
    let (list, list2) = fixture_lists();

    let rad = MTMathAtom::radical();
    assert_eq!(rad.borrow().type_, MTMathAtomType::Radical);
    {
        let mut atom = rad.borrow_mut();
        let radical = atom.as_radical_mut().unwrap();
        radical.radicand = Some(list);
        radical.degree = Some(list2);
    }

    // MTRadical(rad)
    let copy = rad.borrow().copy();
    let test = "test_copy_radical";
    check_atom_copy(Some(&copy), Some(&rad), test);
    let copy_atom = copy.borrow();
    let copy_rad = copy_atom.as_radical().unwrap();
    let rad_atom = rad.borrow();
    let orig_rad = rad_atom.as_radical().unwrap();
    check_list_copy(copy_rad.radicand.as_ref(), orig_rad.radicand.as_ref(), test);
    check_list_copy(copy_rad.degree.as_ref(), orig_rad.degree.as_ref(), test);
}

#[test]
fn test_copy_large_operator() {
    let lg = MTMathAtom::large_operator("lim", true);
    assert_eq!(lg.borrow().type_, MTMathAtomType::LargeOperator);
    assert!(lg.borrow().as_large_operator().unwrap().limits);

    // MTLargeOperator(lg)
    let copy = lg.borrow().copy();
    check_atom_copy(Some(&copy), Some(&lg), "test_copy_large_operator");
    assert_eq!(
        copy.borrow().as_large_operator().unwrap().limits,
        lg.borrow().as_large_operator().unwrap().limits
    );
}

#[test]
fn test_copy_inner() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::times();
    let atom3 = MTMathAtomFactory::divide();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2));
    list.borrow_mut().add(Some(atom3));

    let inner = MTMathAtom::inner();
    inner.borrow_mut().set_inner_list(Some(list));
    {
        let mut atom = inner.borrow_mut();
        let inner = atom.as_inner_mut().unwrap();
        inner.set_left_boundary(Some(MTMathAtom::with_type(MTMathAtomType::Boundary, "(")));
        inner.set_right_boundary(Some(MTMathAtom::with_type(MTMathAtomType::Boundary, ")")));
    }
    assert_eq!(inner.borrow().type_, MTMathAtomType::Inner);

    // MTInner(inner)
    let copy = inner.borrow().copy();
    let test = "test_copy_inner";
    check_atom_copy(Some(&copy), Some(&inner), test);
    check_list_copy(
        copy.borrow().inner_list(),
        inner.borrow().inner_list(),
        test,
    );
    let copy_atom = copy.borrow();
    let copy_inner = copy_atom.as_inner().unwrap();
    let inner_atom = inner.borrow();
    let orig_inner = inner_atom.as_inner().unwrap();
    check_atom_copy(
        Some(copy_inner.left_boundary().unwrap()),
        orig_inner.left_boundary(),
        test,
    );
    check_atom_copy(
        copy_inner.right_boundary(),
        orig_inner.right_boundary(),
        test,
    );
}

#[test]
fn test_set_inner_boundary() {
    let inner = MTMathAtom::inner();

    // Can set non-nil
    {
        let mut atom = inner.borrow_mut();
        let inner = atom.as_inner_mut().unwrap();
        inner.set_left_boundary(Some(MTMathAtom::with_type(MTMathAtomType::Boundary, "(")));
        inner.set_right_boundary(Some(MTMathAtom::with_type(MTMathAtomType::Boundary, ")")));
    }
    assert!(inner.borrow().as_inner().unwrap().left_boundary().is_some());
    assert!(
        inner
            .borrow()
            .as_inner()
            .unwrap()
            .right_boundary()
            .is_some()
    );
    // Can set nil
    {
        let mut atom = inner.borrow_mut();
        let inner = atom.as_inner_mut().unwrap();
        inner.set_left_boundary(None);
        inner.set_right_boundary(None);
    }
    assert!(inner.borrow().as_inner().unwrap().left_boundary().is_none());
    assert!(
        inner
            .borrow()
            .as_inner()
            .unwrap()
            .right_boundary()
            .is_none()
    );
    // Can't set non boundary
    let atom = MTMathAtomFactory::placeholder();
    // XCTExpectFailure("Setting illegal boundary atoms")
    assert_raises(
        || {
            inner
                .borrow_mut()
                .as_inner_mut()
                .unwrap()
                .set_left_boundary(Some(atom.clone()))
        },
        "Left boundary must be of type .boundary",
        "Setting illegal boundary atoms",
    );
    assert_raises(
        || {
            inner
                .borrow_mut()
                .as_inner_mut()
                .unwrap()
                .set_right_boundary(Some(atom.clone()))
        },
        "Right boundary must be of type .boundary",
        "Setting illegal boundary atoms",
    );
}

#[test]
fn test_copy_overline() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::times();
    let atom3 = MTMathAtomFactory::divide();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2));
    list.borrow_mut().add(Some(atom3));

    let over = MTMathAtom::over_line();
    assert_eq!(over.borrow().type_, MTMathAtomType::Overline);
    over.borrow_mut().set_inner_list(Some(list));

    // MTOverLine(over)
    let copy = over.borrow().copy();
    let test = "test_copy_overline";
    check_atom_copy(Some(&copy), Some(&over), test);
    check_list_copy(copy.borrow().inner_list(), over.borrow().inner_list(), test);
}

#[test]
fn test_copy_underline() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::times();
    let atom3 = MTMathAtomFactory::divide();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2));
    list.borrow_mut().add(Some(atom3));

    let under = MTMathAtom::under_line();
    assert_eq!(under.borrow().type_, MTMathAtomType::Underline);
    under.borrow_mut().set_inner_list(Some(list));

    // MTUnderLine(under)
    let copy = under.borrow().copy();
    let test = "test_copy_underline";
    check_atom_copy(Some(&copy), Some(&under), test);
    check_list_copy(
        copy.borrow().inner_list(),
        under.borrow().inner_list(),
        test,
    );
}

#[test]
fn test_copy_acccent() {
    let list = MTMathList::new();
    let atom = MTMathAtomFactory::placeholder();
    let atom2 = MTMathAtomFactory::times();
    let atom3 = MTMathAtomFactory::divide();
    list.borrow_mut().add(Some(atom));
    list.borrow_mut().add(Some(atom2));
    list.borrow_mut().add(Some(atom3));

    let accent = MTMathAtom::accent("^");
    assert_eq!(accent.borrow().type_, MTMathAtomType::Accent);
    accent.borrow_mut().set_inner_list(Some(list));

    // MTAccent(accent)
    let copy = accent.borrow().copy();
    let test = "test_copy_acccent";
    check_atom_copy(Some(&copy), Some(&accent), test);
    check_list_copy(
        copy.borrow().inner_list(),
        accent.borrow().inner_list(),
        test,
    );
}

#[test]
fn test_copy_space() {
    let space = MTMathAtom::space(3.0);
    assert_eq!(space.borrow().type_, MTMathAtomType::Space);

    // MTMathSpace(space)
    let copy = space.borrow().copy();
    check_atom_copy(Some(&copy), Some(&space), "test_copy_space");
    assert_eq!(
        space.borrow().as_space().unwrap().space,
        copy.borrow().as_space().unwrap().space
    );
}

#[test]
fn test_copy_style() {
    let style = MTMathAtom::style(MTLineStyle::Script);
    assert_eq!(style.borrow().type_, MTMathAtomType::Style);

    // MTMathStyle(style)
    let copy = style.borrow().copy();
    check_atom_copy(Some(&copy), Some(&style), "test_copy_style");
    assert_eq!(
        style.borrow().as_style().unwrap().style,
        copy.borrow().as_style().unwrap().style
    );
}

#[test]
fn test_create_math_table() {
    let table = MTMathAtom::table(None);
    assert_eq!(table.borrow().type_, MTMathAtomType::Table);

    let (list, list2) = fixture_lists();

    {
        let mut atom = table.borrow_mut();
        let table = atom.as_table_mut().unwrap();
        table.set_cell(list.clone(), 3, 2);
        table.set_cell(list2.clone(), 1, 0);

        table.set_alignment(MTColumnAlignment::Left, 2);
        table.set_alignment(MTColumnAlignment::Right, 1);
    }

    let atom = table.borrow();
    let table = atom.as_table().unwrap();
    // Verify that everything is created correctly
    assert_eq!(table.cells.len(), 4); // 4 rows
    assert!(!table.cells.is_empty()); // XCTAssertNotNil(table.cells[0])
    assert_eq!(table.cells[0].len(), 0); // 0 elements in row 0
    assert_eq!(table.cells[1].len(), 1); // 1 element in row 1
    assert!(table.cells.get(2).is_some());
    assert_eq!(table.cells[2].len(), 0);
    assert_eq!(table.cells[3].len(), 3);

    // Verify the elements in the rows
    assert_eq!(count(&table.cells[1][0]), 2);
    assert!(Rc::ptr_eq(&table.cells[1][0], &list2));
    assert!(!table.cells[3].is_empty()); // XCTAssertNotNil(table.cells[3][0])
    assert_eq!(count(&table.cells[3][0]), 0);

    assert!(!table.cells[3].is_empty()); // XCTAssertNotNil(table.cells[3][0])
    assert_eq!(count(&table.cells[3][0]), 0);

    assert!(table.cells[3].get(1).is_some());
    assert_eq!(count(&table.cells[3][1]), 0);

    assert!(Rc::ptr_eq(&table.cells[3][2], &list));

    assert_eq!(table.num_rows(), 4);
    assert_eq!(table.num_columns(), 3);

    // Verify the alignments
    assert_eq!(table.alignments.len(), 3);
    assert_eq!(table.alignments[0], MTColumnAlignment::Center);
    assert_eq!(table.alignments[1], MTColumnAlignment::Right);
    assert_eq!(table.alignments[2], MTColumnAlignment::Left);
}

/// Swift `==` on `[MTMathList]`: same count and `isEqual:` (identity) per element.
fn rows_equal(a: &[MTMathListRef], b: &[MTMathListRef]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Rc::ptr_eq(x, y))
}

/// Swift `==` on `[[MTMathList]]`.
fn cells_equal(a: &[Vec<MTMathListRef>], b: &[Vec<MTMathListRef>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| rows_equal(x, y))
}

#[test]
fn test_copy_math_table() {
    let table = MTMathAtom::table(None);
    assert_eq!(table.borrow().type_, MTMathAtomType::Table);

    let (list, list2) = fixture_lists();

    {
        let mut atom = table.borrow_mut();
        let table = atom.as_table_mut().unwrap();
        table.set_cell(list.clone(), 0, 1);
        table.set_cell(list2.clone(), 0, 2);

        table.set_alignment(MTColumnAlignment::Left, 2);
        table.set_alignment(MTColumnAlignment::Right, 1);
        table.inter_row_additional_spacing = 3.0;
        table.inter_column_spacing = 10.0;
    }

    // MTMathTable(table)
    let copy = table.borrow().copy();
    let test = "test_copy_math_table";
    check_atom_copy(Some(&copy), Some(&table), test);
    let copy_atom = copy.borrow();
    let copy_table = copy_atom.as_table().unwrap();
    let table_atom = table.borrow();
    let table = table_atom.as_table().unwrap();
    assert_eq!(copy_table.inter_column_spacing, table.inter_column_spacing);
    assert_eq!(
        copy_table.inter_row_additional_spacing,
        table.inter_row_additional_spacing
    );
    assert_eq!(copy_table.alignments, table.alignments);

    assert!(!cells_equal(&copy_table.cells, &table.cells));
    assert!(!rows_equal(&copy_table.cells[0], &table.cells[0]));
    assert_eq!(copy_table.cells[0].len(), table.cells[0].len());
    assert_eq!(count(&copy_table.cells[0][0]), 0);
    assert!(!Rc::ptr_eq(&copy_table.cells[0][0], &table.cells[0][0]));
    check_list_copy(Some(&copy_table.cells[0][1]), Some(&list), test);
    check_list_copy(Some(&copy_table.cells[0][2]), Some(&list2), test);
}
