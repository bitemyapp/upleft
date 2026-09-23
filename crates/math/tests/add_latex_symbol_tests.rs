//! `MTMathAtomFactory.add(latexSymbol:value:)` stores any atom, by reference.
//!
//! The expectations come from running the same steps against SwiftMath (a
//! scratch package with `@testable import SwiftMath`); each assertion quotes
//! what Swift printed. One function, because the symbol table is global and
//! the steps build on each other.

use upleft_math::math_render::mt_math_atom_factory::MTMathAtomFactory;
use upleft_math::math_render::mt_math_list::{MTMathAtom, MTMathAtomType, MTMathListRef};
use upleft_math::math_render::mt_math_list_builder::MTMathListBuilder;

/// `"\(type.rawValue):\(type(of: atom)):\(atom.description)"` per atom.
fn atoms(list: &MTMathListRef) -> Vec<String> {
    list.borrow()
        .atoms
        .iter()
        .map(|atom| {
            let atom = atom.borrow();
            format!(
                "{}:{}:{}",
                atom.type_.raw_value(),
                atom.kind.class_name(),
                atom.description()
            )
        })
        .collect()
}

fn build(latex: &str) -> MTMathListRef {
    let mut error = None;
    let list = MTMathListBuilder::build_from_string_with_error(latex, &mut error);
    assert!(error.is_none(), "{latex}: {error:?}");
    list.unwrap()
}

#[test]
fn any_atom_can_be_registered() {
    // A fraction registered as a symbol.
    let frac = MTMathAtomFactory::fraction_with_strings("1", "2");
    MTMathAtomFactory::add_latex_symbol("half", &frac);
    let list = build("x\\half y");
    assert_eq!(
        atoms(&list),
        [
            "3:MTMathAtom:x",
            "10:MTFraction:\\frac{[1]}{[2]}",
            "3:MTMathAtom:y"
        ]
    );
    assert_eq!(
        MTMathListBuilder::math_list_to_string(Some(&list)),
        "x\\frac{1}{2}y"
    );

    // The table holds the caller's atom: a later change shows in lookups.
    frac.borrow_mut().as_fraction_mut().unwrap().numerator =
        Some(MTMathAtomFactory::atom_list_for("3"));
    let list = build("\\half");
    assert_eq!(atoms(&list), ["10:MTFraction:\\frac{[3]}{[2]}"]);
    assert_eq!(
        MTMathListBuilder::math_list_to_string(Some(&list)),
        "\\frac{3}{2}"
    );

    // Lookups hand out copies.
    let a1 = MTMathAtomFactory::atom_for_latex_symbol("half").unwrap();
    let a2 = MTMathAtomFactory::atom_for_latex_symbol("half").unwrap();
    assert!(!std::rc::Rc::ptr_eq(&a1, &a2) && !std::rc::Rc::ptr_eq(&a1, &frac));

    // An inner with no boundaries copies them as empty ordinary atoms.
    let inner = MTMathAtom::inner();
    inner
        .borrow_mut()
        .set_inner_list(Some(MTMathAtomFactory::atom_list_for("ab")));
    MTMathAtomFactory::add_latex_symbol("grp", &inner);
    let list = build("\\grp^2");
    assert_eq!(atoms(&list), ["14:MTInner:\\inner[]{[a, b]}[]^{[2]}"]);
    assert_eq!(
        MTMathListBuilder::math_list_to_string(Some(&list)),
        "\\left ab\\right ^{2}"
    );

    // A plain atom whose type is .fraction copies as an empty MTFraction.
    let odd = MTMathAtom::with_type(MTMathAtomType::Fraction, "q");
    MTMathAtomFactory::add_latex_symbol("odd", &odd);
    let list = build("\\odd");
    {
        let atom = list.borrow().atoms[0].clone();
        let atom = atom.borrow();
        assert_eq!(atom.type_.raw_value(), 10);
        assert_eq!(atom.kind.class_name(), "MTFraction");
        assert!(atom.as_fraction().unwrap().numerator.is_none());
        assert!(atom.nucleus.is_empty());
    }

    // A radical.
    MTMathAtomFactory::add_latex_symbol("rt", &MTMathAtomFactory::placeholder_square_root());
    let list = build("\\rt");
    assert_eq!(atoms(&list), ["11:MTRadical:\\sqrt{[\u{25A1}]}"]);
    assert_eq!(
        MTMathListBuilder::math_list_to_string(Some(&list)),
        "\\sqrt{\\square }"
    );

    // The reverse map records the nucleus at registration ("" for a fraction).
    assert_eq!(MTMathAtomFactory::latex_symbol_name(&frac.borrow()), None);

    // Replacing a built-in.
    MTMathAtomFactory::add_latex_symbol(
        "alpha",
        &MTMathAtom::with_type(MTMathAtomType::Relation, "A"),
    );
    let list = build("\\alpha");
    assert_eq!(atoms(&list), ["7:MTMathAtom:A"]);
    assert_eq!(
        MTMathListBuilder::math_list_to_string(Some(&list)),
        "\\alpha "
    );

    // A table.
    let mut error = None;
    let rows = vec![vec![
        MTMathAtomFactory::atom_list_for("a"),
        MTMathAtomFactory::atom_list_for("b"),
    ]];
    let table = MTMathAtomFactory::table(Some("matrix"), &rows, &mut error).unwrap();
    MTMathAtomFactory::add_latex_symbol("mat", &table);
    let list = build("\\mat");
    assert_eq!(atoms(&list), ["1001:MTMathTable:"]);
    assert_eq!(
        MTMathListBuilder::math_list_to_string(Some(&list)),
        "\\begin{matrix}a&b\\end{matrix}"
    );

    // Another thread copies the atom as it was registered: it cannot reach
    // the caller's cells. (In Swift an unsynchronised cross-thread read is a
    // data race; with synchronisation it would see the change.)
    let late = MTMathAtomFactory::fraction_with_strings("5", "6");
    MTMathAtomFactory::add_latex_symbol("late", &late);
    late.borrow_mut().as_fraction_mut().unwrap().numerator =
        Some(MTMathAtomFactory::atom_list_for("7"));
    assert_eq!(atoms(&build("\\late")), ["10:MTFraction:\\frac{[7]}{[6]}"]);
    let elsewhere = std::thread::spawn(|| atoms(&build("\\late")))
        .join()
        .unwrap();
    assert_eq!(elsewhere, ["10:MTFraction:\\frac{[5]}{[6]}"]);
}
