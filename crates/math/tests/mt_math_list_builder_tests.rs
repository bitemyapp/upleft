//! Port of SwiftMath's `Tests/SwiftMathTests/MTMathListBuilderTests.swift`
//! (as vendored by Downright). Every Swift test function, assertion and data
//! table is kept, in the same order.

use upleft_math::math_render::mt_math_atom_factory::MTMathAtomFactory;
use upleft_math::math_render::mt_math_list::{
    MTColumnAlignment, MTFontStyle, MTLineStyle, MTMathAtomRef, MTMathAtomType,
    MTMathAtomType as T, MTMathListRef,
};
use upleft_math::math_render::mt_math_list_builder::{
    MT_PARSE_ERROR, MTMathListBuilder, MTParseError, MTParseErrors,
};

fn check_atom_types(list: Option<&MTMathListRef>, types: &[MTMathAtomType], desc: &str) {
    if let Some(list) = list {
        let list = list.borrow();
        assert_eq!(list.atoms.len(), types.len(), "{desc}");
        for i in 0..list.atoms.len() {
            let atom = &list.atoms[i];
            assert_eq!(atom.borrow().type_, types[i], "{desc}");
        }
    } else {
        assert!(types.is_empty(), "MathList should have no atoms!");
    }
}

/// `list.atoms[i]`.
fn atom_at(list: &MTMathListRef, i: usize) -> MTMathAtomRef {
    list.borrow().atoms[i].clone()
}

/// `list.atoms.count`.
fn count(list: &MTMathListRef) -> usize {
    list.borrow().atoms.len()
}

struct TestRecord {
    build: &'static str,
    atom_type: &'static [MTMathAtomType],
    types: &'static [MTMathAtomType],
    extra: &'static [MTMathAtomType],
    result: &'static str,
}

impl TestRecord {
    const fn new(
        build: &'static str,
        atom_type: &'static [MTMathAtomType],
        types: &'static [MTMathAtomType],
        result: &'static str,
    ) -> Self {
        TestRecord {
            build,
            atom_type,
            types,
            extra: &[],
            result,
        }
    }

    const fn with_extra(
        build: &'static str,
        atom_type: &'static [MTMathAtomType],
        types: &'static [MTMathAtomType],
        extra: &'static [MTMathAtomType],
        result: &'static str,
    ) -> Self {
        TestRecord {
            build,
            atom_type,
            types,
            extra,
            result,
        }
    }
}

fn get_test_data() -> Vec<TestRecord> {
    vec![
        TestRecord::new("x", &[T::Variable], &[], "x"),
        TestRecord::new("1", &[T::Number], &[], "1"),
        TestRecord::new("*", &[T::BinaryOperator], &[], "*"),
        TestRecord::new("+", &[T::BinaryOperator], &[], "+"),
        TestRecord::new(".", &[T::Number], &[], "."),
        TestRecord::new("(", &[T::Open], &[], "("),
        TestRecord::new(")", &[T::Close], &[], ")"),
        TestRecord::new(",", &[T::Punctuation], &[], ","),
        TestRecord::new("!", &[T::Close], &[], "!"),
        TestRecord::new("=", &[T::Relation], &[], "="),
        TestRecord::new(
            "x+2",
            &[T::Variable, T::BinaryOperator, T::Number],
            &[],
            "x+2",
        ),
        // spaces are ignored
        TestRecord::new(
            "(2.3 * 8)",
            &[
                T::Open,
                T::Number,
                T::Number,
                T::Number,
                T::BinaryOperator,
                T::Number,
                T::Close,
            ],
            &[],
            "(2.3*8)",
        ),
        // braces are just for grouping
        TestRecord::new(
            "5{3+4}",
            &[T::Number, T::Number, T::BinaryOperator, T::Number],
            &[],
            "53+4",
        ),
        // commands
        TestRecord::new(
            "\\pi+\\theta\\geq 3",
            &[
                T::Variable,
                T::BinaryOperator,
                T::Variable,
                T::Relation,
                T::Number,
            ],
            &[],
            "\\pi +\\theta \\geq 3",
        ),
        // aliases
        TestRecord::new(
            "\\pi\\ne 5 \\land 3",
            &[
                T::Variable,
                T::Relation,
                T::Number,
                T::BinaryOperator,
                T::Number,
            ],
            &[],
            "\\pi \\neq 5\\wedge 3",
        ),
        // control space
        TestRecord::new(
            "x \\ y",
            &[T::Variable, T::Ordinary, T::Variable],
            &[],
            "x\\  y",
        ),
        // spacing
        TestRecord::new(
            "x \\quad y \\; z \\! q",
            &[
                T::Variable,
                T::Space,
                T::Variable,
                T::Space,
                T::Variable,
                T::Space,
                T::Variable,
            ],
            &[],
            "x\\quad y\\; z\\! q",
        ),
    ]
}

fn get_test_data_super_script() -> Vec<TestRecord> {
    vec![
        TestRecord::new("x^2", &[T::Variable], &[T::Number], "x^{2}"),
        TestRecord::new("x^23", &[T::Variable, T::Number], &[T::Number], "x^{2}3"),
        TestRecord::new("x^{23}", &[T::Variable], &[T::Number, T::Number], "x^{23}"),
        TestRecord::new(
            "x^2^3",
            &[T::Variable, T::Ordinary],
            &[T::Number],
            "x^{2}{}^{3}",
        ),
        TestRecord::with_extra(
            "x^{2^3}",
            &[T::Variable],
            &[T::Number],
            &[T::Number],
            "x^{2^{3}}",
        ),
        TestRecord::with_extra(
            "x^{^2*}",
            &[T::Variable],
            &[T::Ordinary, T::BinaryOperator],
            &[T::Number],
            "x^{{}^{2}*}",
        ),
        TestRecord::new("^2", &[T::Ordinary], &[T::Number], "{}^{2}"),
        TestRecord::new("{}^2", &[T::Ordinary], &[T::Number], "{}^{2}"),
        TestRecord::new("x^^2", &[T::Variable, T::Ordinary], &[], "x^{}{}^{2}"),
        TestRecord::new("5{x}^2", &[T::Number, T::Variable], &[], "5x^{2}"),
    ]
}

fn get_test_data_sub_script() -> Vec<TestRecord> {
    vec![
        TestRecord::new("x_2", &[T::Variable], &[T::Number], "x_{2}"),
        TestRecord::new("x_23", &[T::Variable, T::Number], &[T::Number], "x_{2}3"),
        TestRecord::new("x_{23}", &[T::Variable], &[T::Number, T::Number], "x_{23}"),
        TestRecord::new(
            "x_2_3",
            &[T::Variable, T::Ordinary],
            &[T::Number],
            "x_{2}{}_{3}",
        ),
        TestRecord::with_extra(
            "x_{2_3}",
            &[T::Variable],
            &[T::Number],
            &[T::Number],
            "x_{2_{3}}",
        ),
        TestRecord::with_extra(
            "x_{_2*}",
            &[T::Variable],
            &[T::Ordinary, T::BinaryOperator],
            &[T::Number],
            "x_{{}_{2}*}",
        ),
        TestRecord::new("_2", &[T::Ordinary], &[T::Number], "{}_{2}"),
        TestRecord::new("{}_2", &[T::Ordinary], &[T::Number], "{}_{2}"),
        TestRecord::new("x__2", &[T::Variable, T::Ordinary], &[], "x_{}{}_{2}"),
        TestRecord::new("5{x}_2", &[T::Number, T::Variable], &[], "5x_{2}"),
    ]
}

fn get_test_data_super_sub_script() -> Vec<TestRecord> {
    vec![
        TestRecord::with_extra(
            "x_2^*",
            &[T::Variable],
            &[T::Number],
            &[T::BinaryOperator],
            "x^{*}_{2}",
        ),
        TestRecord::with_extra(
            "x^*_2",
            &[T::Variable],
            &[T::Number],
            &[T::BinaryOperator],
            "x^{*}_{2}",
        ),
        TestRecord::with_extra(
            "x_^*",
            &[T::Variable],
            &[],
            &[T::BinaryOperator],
            "x^{*}_{}",
        ),
        TestRecord::new("x^_2", &[T::Variable], &[T::Number], "x^{}_{2}"),
        TestRecord::new("x_{2^*}", &[T::Variable], &[T::Number], "x_{2^{*}}"),
        TestRecord::with_extra(
            "x^{*_2}",
            &[T::Variable],
            &[],
            &[T::BinaryOperator],
            "x^{*_{2}}",
        ),
        TestRecord::with_extra(
            "_2^*",
            &[T::Ordinary],
            &[T::Number],
            &[T::BinaryOperator],
            "{}^{*}_{2}",
        ),
    ]
}

struct TestRecord2 {
    build: &'static str,
    type1: &'static [MTMathAtomType],
    number: usize,
    type2: &'static [MTMathAtomType],
    left: &'static str,
    right: &'static str,
    result: &'static str,
}

fn get_test_data_left_right() -> Vec<TestRecord2> {
    vec![
        TestRecord2 {
            build: "\\left( 2 \\right)",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number],
            left: "(",
            right: ")",
            result: "\\left( 2\\right) ",
        },
        // spacing
        TestRecord2 {
            build: "\\left ( 2 \\right )",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number],
            left: "(",
            right: ")",
            result: "\\left( 2\\right) ",
        },
        // commands
        TestRecord2 {
            build: "\\left\\{ 2 \\right\\}",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number],
            left: "{",
            right: "}",
            result: "\\left\\{ 2\\right\\} ",
        },
        // complex commands
        TestRecord2 {
            build: "\\left\\langle x \\right\\rangle",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Variable],
            left: "\u{2329}",
            right: "\u{232A}",
            result: "\\left< x\\right> ",
        },
        // bars
        TestRecord2 {
            build: "\\left| x \\right\\|",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Variable],
            left: "|",
            right: "\u{2016}",
            result: "\\left| x\\right\\| ",
        },
        // inner in between
        TestRecord2 {
            build: "5 + \\left( 2 \\right) - 2",
            type1: &[
                T::Number,
                T::BinaryOperator,
                T::Inner,
                T::BinaryOperator,
                T::Number,
            ],
            number: 2,
            type2: &[T::Number],
            left: "(",
            right: ")",
            result: "5+\\left( 2\\right) -2",
        },
        // long inner
        TestRecord2 {
            build: "\\left( 2 + \\frac12\\right)",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number, T::BinaryOperator, T::Fraction],
            left: "(",
            right: ")",
            result: "\\left( 2+\\frac{1}{2}\\right) ",
        },
        // nested
        TestRecord2 {
            build: "\\left[ 2 + \\left|\\frac{-x}{2}\\right| \\right]",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number, T::BinaryOperator, T::Inner],
            left: "[",
            right: "]",
            result: "\\left[ 2+\\left| \\frac{-x}{2}\\right| \\right] ",
        },
        // With scripts
        TestRecord2 {
            build: "\\left( 2 \\right)^2",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number],
            left: "(",
            right: ")",
            result: "\\left( 2\\right) ^{2}",
        },
        // Scripts on left
        TestRecord2 {
            build: "\\left(^2 \\right )",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Ordinary],
            left: "(",
            right: ")",
            result: "\\left( {}^{2}\\right) ",
        },
        // Dot
        TestRecord2 {
            build: "\\left( 2 \\right.",
            type1: &[T::Inner],
            number: 0,
            type2: &[T::Number],
            left: "(",
            right: "",
            result: "\\left( 2\\right. ",
        },
    ]
}

fn get_test_data_parse_errors() -> Vec<(&'static str, MTParseErrors)> {
    use MTParseErrors as E;
    vec![
        ("}a", E::MismatchBraces),
        ("\\notacommand", E::InvalidCommand),
        ("\\sqrt[5+3", E::CharacterNotFound),
        ("{5+3", E::MismatchBraces),
        ("5+3}", E::MismatchBraces),
        ("{1+\\frac{3+2", E::MismatchBraces),
        ("1+\\left", E::MissingDelimiter),
        ("\\left(\\frac12\\right", E::MissingDelimiter),
        ("\\left 5 + 3 \\right)", E::InvalidDelimiter),
        ("\\left(\\frac12\\right + 3", E::InvalidDelimiter),
        ("\\left\\lmoustache 5 + 3 \\right)", E::InvalidDelimiter),
        (
            "\\left(\\frac12\\right\\rmoustache + 3",
            E::InvalidDelimiter,
        ),
        ("5 + 3 \\right)", E::MissingLeft),
        ("\\left(\\frac12", E::MissingRight),
        ("\\left(5 + \\left| \\frac12 \\right)", E::MissingRight),
        ("5+ \\left|\\frac12\\right| \\right)", E::MissingLeft),
        ("\\begin matrix \\end matrix", E::CharacterNotFound), // missing {
        ("\\begin", E::CharacterNotFound),                     // missing {
        ("\\begin{", E::CharacterNotFound),                    // missing }
        ("\\begin{matrix parens}", E::CharacterNotFound),      // missing } (no spaces in env)
        ("\\begin{matrix} x", E::MissingEnd),
        ("\\begin{matrix} x \\end", E::CharacterNotFound), // missing {
        ("\\begin{matrix} x \\end + 3", E::CharacterNotFound), // missing {
        ("\\begin{matrix} x \\end{", E::CharacterNotFound), // missing }
        ("\\begin{matrix} x \\end{matrix + 3", E::CharacterNotFound), // missing }
        ("\\begin{matrix} x \\end{pmatrix}", E::InvalidEnv),
        ("x \\end{matrix}", E::MissingBegin),
        ("\\begin{notanenv} x \\end{notanenv}", E::InvalidEnv),
        (
            "\\begin{matrix} \\notacommand \\end{matrix}",
            E::InvalidCommand,
        ),
        (
            "\\begin{displaylines} x & y \\end{displaylines}",
            E::InvalidNumColumns,
        ),
        ("\\begin{eqalign} x \\end{eqalign}", E::InvalidNumColumns),
        ("\\nolimits", E::InvalidLimits),
        ("\\frac\\limits{1}{2}", E::InvalidLimits),
        ("&\\begin", E::CharacterNotFound),
        ("x & y \\\\ z & w \\end{matrix}", E::InvalidEnv),
    ]
}

#[test]
fn test_builder() {
    let data = get_test_data();
    for test_case in &data {
        let str = test_case.build;
        let mut error: Option<MTParseError> = None;
        let list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
        assert!(error.is_none(), "{error:?}");
        let desc = format!("Error for string:{str}");
        let atom_types = test_case.atom_type;
        check_atom_types(list.as_ref(), atom_types, &desc);

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(list.as_ref());
        assert_eq!(latex, test_case.result, "{desc}");
    }
}

#[test]
fn test_super_script() {
    let data = get_test_data_super_script();
    for test_case in &data {
        let str = test_case.build;
        let mut error: Option<MTParseError> = None;
        let list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
        assert!(error.is_none(), "{error:?}");
        let desc = format!("Error for string:{str}");
        let atom_types = test_case.atom_type;
        check_atom_types(list.as_ref(), atom_types, &desc);

        // get the first atom
        let first = atom_at(list.as_ref().unwrap(), 0);
        // check it's superscript
        let types = test_case.types;
        if !types.is_empty() {
            assert!(first.borrow().super_script().is_some(), "{desc}");
        }
        let superlist = first.borrow().super_script().cloned();
        check_atom_types(superlist.as_ref(), types, &desc);

        if !test_case.extra.is_empty() {
            // one more level
            let super_first = atom_at(superlist.as_ref().unwrap(), 0);
            let supersuper_list = super_first.borrow().super_script().cloned();
            check_atom_types(supersuper_list.as_ref(), test_case.extra, &desc);
        }

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(list.as_ref());
        assert_eq!(latex, test_case.result, "{desc}");
    }
}

#[test]
fn test_sub_script() {
    let data = get_test_data_sub_script();
    for test_case in &data {
        let str = test_case.build;
        let mut error: Option<MTParseError> = None;
        let list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
        assert!(error.is_none(), "{error:?}");
        let desc = format!("Error for string:{str}");
        let atom_types = test_case.atom_type;
        check_atom_types(list.as_ref(), atom_types, &desc);

        // get the first atom
        let first = atom_at(list.as_ref().unwrap(), 0);
        // check it's superscript
        let types = test_case.types;
        if !types.is_empty() {
            assert!(first.borrow().sub_script().is_some(), "{desc}");
        }
        let sublist = first.borrow().sub_script().cloned();
        check_atom_types(sublist.as_ref(), types, &desc);

        if !test_case.extra.is_empty() {
            // one more level
            let sub_first = atom_at(sublist.as_ref().unwrap(), 0);
            let subsub_list = sub_first.borrow().sub_script().cloned();
            check_atom_types(subsub_list.as_ref(), test_case.extra, &desc);
        }

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(list.as_ref());
        assert_eq!(latex, test_case.result, "{desc}");
    }
}

#[test]
fn test_super_sub_script() {
    let data = get_test_data_super_sub_script();
    for test_case in &data {
        let str = test_case.build;
        let mut error: Option<MTParseError> = None;
        let list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
        assert!(error.is_none(), "{error:?}");
        let desc = format!("Error for string:{str}");
        let atom_types = test_case.atom_type;
        check_atom_types(list.as_ref(), atom_types, &desc);

        // get the first atom
        let first = atom_at(list.as_ref().unwrap(), 0);
        // check its subscript
        let sub = test_case.types;
        if !sub.is_empty() {
            assert!(first.borrow().sub_script().is_some(), "{desc}");
            let sublist = first.borrow().sub_script().cloned();
            check_atom_types(sublist.as_ref(), sub, &desc);
        }
        let sup = test_case.extra;
        if !sup.is_empty() {
            assert!(first.borrow().super_script().is_some(), "{desc}");
            let sublist = first.borrow().super_script().cloned();
            check_atom_types(sublist.as_ref(), sup, &desc);
        }

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(list.as_ref());
        assert_eq!(latex, test_case.result, "{desc}");
    }
}

#[test]
fn test_symbols() {
    let str = "5\\times3^{2\\div2}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 3, "{desc}");
    let mut atom = atom_at(&list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "5", "{desc}");
    atom = atom_at(&list, 1);
    assert_eq!(atom.borrow().type_, T::BinaryOperator, "{desc}");
    assert_eq!(atom.borrow().nucleus, "\u{00D7}", "{desc}");
    atom = atom_at(&list, 2);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "3", "{desc}");

    // super script
    let super_list = atom.borrow().super_script().cloned().unwrap();
    assert_eq!(count(&super_list), 3, "{desc}");
    atom = atom_at(&super_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");
    atom = atom_at(&super_list, 1);
    assert_eq!(atom.borrow().type_, T::BinaryOperator, "{desc}");
    assert_eq!(atom.borrow().nucleus, "\u{00F7}", "{desc}");
    atom = atom_at(&super_list, 2);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_frac() {
    let str = "\\frac1c";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(frac.has_rule);
    assert!(frac.right_delimiter.is_empty());
    assert!(frac.left_delimiter.is_empty());

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "1", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "c", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\frac{1}{c}", "{desc}");
}

#[test]
fn test_frac_in_frac() {
    let str = "\\frac1\\frac23";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let mut frac_atom = atom_at(&list, 0);
    assert!(frac_atom.borrow().as_fraction().is_some(), "MTFraction");
    assert_eq!(frac_atom.borrow().type_, T::Fraction, "{desc}");
    assert_eq!(frac_atom.borrow().nucleus, "", "{desc}");
    assert!(frac_atom.borrow().as_fraction().unwrap().has_rule);

    let mut sub_list = frac_atom
        .borrow()
        .as_fraction()
        .unwrap()
        .numerator
        .clone()
        .unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "1", "{desc}");

    sub_list = frac_atom
        .borrow()
        .as_fraction()
        .unwrap()
        .denominator
        .clone()
        .unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    frac_atom = atom_at(&sub_list, 0);
    assert!(frac_atom.borrow().as_fraction().is_some(), "MTFraction");
    assert_eq!(frac_atom.borrow().type_, T::Fraction, "{desc}");
    assert_eq!(frac_atom.borrow().nucleus, "", "{desc}");

    sub_list = frac_atom
        .borrow()
        .as_fraction()
        .unwrap()
        .numerator
        .clone()
        .unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");

    sub_list = frac_atom
        .borrow()
        .as_fraction()
        .unwrap()
        .denominator
        .clone()
        .unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "3", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\frac{1}{\\frac{2}{3}}", "{desc}");
}

#[test]
fn test_sqrt() {
    let str = "\\sqrt2";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let rad_atom = atom_at(&list, 0);
    let rad_ref = rad_atom.borrow();
    let rad = rad_ref.as_radical().expect("MTRadical");
    assert_eq!(rad_ref.type_, T::Radical, "{desc}");
    assert_eq!(rad_ref.nucleus, "", "{desc}");

    let sub_list = rad.radicand.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sqrt{2}", "{desc}");
}

#[test]
fn test_sqrt_in_sqrt() {
    let str = "\\sqrt\\sqrt2";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let mut rad_atom = atom_at(&list, 0);
    assert!(rad_atom.borrow().as_radical().is_some(), "MTRadical");
    assert_eq!(rad_atom.borrow().type_, T::Radical, "{desc}");
    assert_eq!(rad_atom.borrow().nucleus, "", "{desc}");

    let mut sub_list = rad_atom
        .borrow()
        .as_radical()
        .unwrap()
        .radicand
        .clone()
        .unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    rad_atom = atom_at(&sub_list, 0);
    assert!(rad_atom.borrow().as_radical().is_some(), "MTRadical");
    assert_eq!(rad_atom.borrow().type_, T::Radical, "{desc}");
    assert_eq!(rad_atom.borrow().nucleus, "", "{desc}");

    sub_list = rad_atom
        .borrow()
        .as_radical()
        .unwrap()
        .radicand
        .clone()
        .unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sqrt{\\sqrt{2}}", "{desc}");
}

#[test]
fn test_rad() {
    let str = "\\sqrt[3]2";
    let list = MTMathListBuilder::build_from_string(str).unwrap();

    assert_eq!(count(&list), 1);
    let rad_atom = atom_at(&list, 0);
    let rad_ref = rad_atom.borrow();
    let rad = rad_ref.as_radical().expect("MTRadical");
    assert_eq!(rad_ref.type_, T::Radical);
    assert_eq!(rad_ref.nucleus, "");

    let mut sub_list = rad.radicand.clone().unwrap();
    assert_eq!(count(&sub_list), 1);
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number);
    assert_eq!(atom.borrow().nucleus, "2");

    sub_list = rad.degree.clone().unwrap();
    assert_eq!(count(&sub_list), 1);
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number);
    assert_eq!(atom.borrow().nucleus, "3");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sqrt[3]{2}");
}

#[test]
fn test_sqrt_without_radicand() {
    let str = "\\sqrt";
    let list = MTMathListBuilder::build_from_string(str).expect("XCTUnwrap");

    assert_eq!(count(&list), 1);
    let first = list.borrow().atoms.first().cloned().expect("XCTUnwrap");
    let rad_ref = first.borrow();
    let rad = rad_ref.as_radical().expect("XCTUnwrap");
    assert_eq!(rad_ref.type_, T::Radical);
    assert_eq!(rad_ref.nucleus, "");

    assert_eq!(
        rad.radicand.as_ref().map(|r| r.borrow().atoms.is_empty()),
        Some(true)
    );
    assert!(rad.degree.is_none());

    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sqrt{}");
}

#[test]
fn test_sqrt_with_degree_without_radicand() {
    let str = "\\sqrt[3]";
    let list = MTMathListBuilder::build_from_string(str).expect("XCTUnwrap");

    assert_eq!(count(&list), 1);
    let first = list.borrow().atoms.first().cloned().expect("XCTUnwrap");
    let rad_ref = first.borrow();
    let rad = rad_ref.as_radical().expect("XCTUnwrap");
    assert_eq!(rad_ref.type_, T::Radical);
    assert_eq!(rad_ref.nucleus, "");

    assert_eq!(
        rad.radicand.as_ref().map(|r| r.borrow().atoms.is_empty()),
        Some(true)
    );

    let sub_list = rad.degree.clone().expect("XCTUnwrap");
    assert_eq!(count(&sub_list), 1);
    let atom = sub_list.borrow().atoms.first().cloned().expect("XCTUnwrap");
    assert_eq!(atom.borrow().type_, T::Number);
    assert_eq!(atom.borrow().nucleus, "3");

    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sqrt[3]{}");
}

#[test]
fn test_left_right() {
    let data = get_test_data_left_right();
    for test_case in &data {
        let str = test_case.build;

        let mut error: Option<MTParseError> = None;
        let list = MTMathListBuilder::build_from_string_with_error(str, &mut error).unwrap();

        assert!(error.is_none(), "{str}");

        check_atom_types(Some(&list), test_case.type1, &format!("{str} outer"));

        let inner_loc = test_case.number;
        let inner_atom = atom_at(&list, inner_loc);
        let inner_ref = inner_atom.borrow();
        let inner = inner_ref.as_inner().expect("MTInner");
        assert_eq!(inner_ref.type_, T::Inner, "{str}");
        assert_eq!(inner_ref.nucleus, "", "{str}");

        let inner_list = inner.inner_list.clone().unwrap();
        check_atom_types(Some(&inner_list), test_case.type2, &format!("{str} inner"));

        assert!(inner.left_boundary().is_some(), "{str}");
        assert_eq!(
            inner.left_boundary().unwrap().borrow().type_,
            T::Boundary,
            "{str}"
        );
        assert_eq!(
            inner.left_boundary().unwrap().borrow().nucleus,
            test_case.left,
            "{str}"
        );

        assert!(inner.right_boundary().is_some(), "{str}");
        assert_eq!(
            inner.right_boundary().unwrap().borrow().type_,
            T::Boundary,
            "{str}"
        );
        assert_eq!(
            inner.right_boundary().unwrap().borrow().nucleus,
            test_case.right,
            "{str}"
        );

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(Some(&list));
        assert_eq!(latex, test_case.result, "{str}");
    }
}

#[test]
#[allow(unused_assignments)]
fn test_over() {
    let str = "1 \\over c";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(frac.has_rule);
    assert!(frac.right_delimiter.is_empty());
    assert!(frac.left_delimiter.is_empty());

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "1", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "c", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\frac{1}{c}", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_over_in_parens() {
    let str = "5 + {1 \\over c} + 8";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 5, "{desc}");
    let types = [
        T::Number,
        T::BinaryOperator,
        T::Fraction,
        T::BinaryOperator,
        T::Number,
    ];
    check_atom_types(Some(&list), &types, &desc);

    let frac_atom = atom_at(&list, 2);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(frac.has_rule);
    assert!(frac.right_delimiter.is_empty());
    assert!(frac.left_delimiter.is_empty());

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "1", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "c", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "5+\\frac{1}{c}+8", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_atop() {
    let str = "1 \\atop c";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(!frac.has_rule);
    assert!(frac.right_delimiter.is_empty());
    assert!(frac.left_delimiter.is_empty());

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "1", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "c", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "{1 \\atop c}", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_atop_in_parens() {
    let str = "5 + {1 \\atop c} + 8";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 5, "{desc}");
    let types = [
        T::Number,
        T::BinaryOperator,
        T::Fraction,
        T::BinaryOperator,
        T::Number,
    ];
    check_atom_types(Some(&list), &types, &desc);

    let frac_atom = atom_at(&list, 2);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(!frac.has_rule);
    assert!(frac.right_delimiter.is_empty());
    assert!(frac.left_delimiter.is_empty());

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "1", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "c", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "5+{1 \\atop c}+8", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_choose() {
    let str = "n \\choose k";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(!frac.has_rule);
    assert_eq!(frac.right_delimiter, ")");
    assert_eq!(frac.left_delimiter, "(");

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "n", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "k", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "{n \\choose k}", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_brack() {
    let str = "n \\brack k";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(!frac.has_rule);
    assert_eq!(frac.right_delimiter, "]");
    assert_eq!(frac.left_delimiter, "[");

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "n", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "k", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "{n \\brack k}", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_brace() {
    let str = "n \\brace k";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(!frac.has_rule);
    assert_eq!(frac.right_delimiter, "}");
    assert_eq!(frac.left_delimiter, "{");

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "n", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "k", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "{n \\brace k}", "{desc}");
}

#[test]
#[allow(unused_assignments)]
fn test_binom() {
    let str = "\\binom{n}{k}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let frac_atom = atom_at(&list, 0);
    let frac_ref = frac_atom.borrow();
    let frac = frac_ref.as_fraction().expect("MTFraction");
    assert_eq!(frac_ref.type_, T::Fraction, "{desc}");
    assert_eq!(frac_ref.nucleus, "", "{desc}");
    assert!(!frac.has_rule);
    assert_eq!(frac.right_delimiter, ")");
    assert_eq!(frac.left_delimiter, "(");

    let mut sub_list = frac.numerator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "n", "{desc}");

    atom = atom_at(&list, 0);
    sub_list = frac.denominator.clone().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "k", "{desc}");

    // convert it back to latex (binom converts to choose)
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "{n \\choose k}", "{desc}");
}

#[test]
fn test_over_line() {
    let str = "\\overline 2";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let over = atom_at(&list, 0);
    assert_eq!(over.borrow().kind.class_name(), "MTOverLine");
    assert_eq!(over.borrow().type_, T::Overline, "{desc}");
    assert_eq!(over.borrow().nucleus, "", "{desc}");

    let sub_list = over.borrow().inner_list().cloned().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\overline{2}", "{desc}");
}

#[test]
fn test_underline() {
    let str = "\\underline 2";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let under = atom_at(&list, 0);
    assert_eq!(under.borrow().kind.class_name(), "MTUnderLine");
    assert_eq!(under.borrow().type_, T::Underline, "{desc}");
    assert_eq!(under.borrow().nucleus, "", "{desc}");

    let sub_list = under.borrow().inner_list().cloned().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Number, "{desc}");
    assert_eq!(atom.borrow().nucleus, "2", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\underline{2}", "{desc}");
}

#[test]
fn test_accent() {
    let str = "\\bar x";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let accent = atom_at(&list, 0);
    assert_eq!(accent.borrow().kind.class_name(), "MTAccent");
    assert_eq!(accent.borrow().type_, T::Accent, "{desc}");
    assert_eq!(accent.borrow().nucleus, "\u{0304}", "{desc}");

    let sub_list = accent.borrow().inner_list().cloned().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "x", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\bar{x}", "{desc}");
}

#[test]
fn test_accented_character() {
    // U+00E1 LATIN SMALL LETTER A WITH ACUTE, precomposed as in the Swift source.
    let str = "\u{00E1}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let accent = atom_at(&list, 0);
    assert_eq!(accent.borrow().kind.class_name(), "MTAccent");
    assert_eq!(accent.borrow().type_, T::Accent, "{desc}");
    assert_eq!(accent.borrow().nucleus, "\u{0301}", "{desc}");

    let sub_list = accent.borrow().inner_list().cloned().unwrap();
    assert_eq!(count(&sub_list), 1, "{desc}");
    let atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "a", "{desc}");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\acute{a}", "{desc}");
}

#[test]
fn test_math_space() {
    let str = "\\!";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let space_atom = atom_at(&list, 0);
    let space_ref = space_atom.borrow();
    let space = space_ref.as_space().expect("MTMathSpace");
    assert_eq!(space_ref.type_, T::Space, "{desc}");
    assert_eq!(space_ref.nucleus, "", "{desc}");
    assert_eq!(space.space, -3.0);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\! ", "{desc}");
}

#[test]
fn test_math_style() {
    let str = "\\textstyle y \\scriptstyle x";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 4, "{desc}");
    let style_atom = atom_at(&list, 0);
    let style_ref = style_atom.borrow();
    let style = style_ref.as_style().expect("MTMathStyle");
    assert_eq!(style_ref.type_, T::Style, "{desc}");
    assert_eq!(style_ref.nucleus, "", "{desc}");
    assert_eq!(style.style, MTLineStyle::Text);

    let style2_atom = atom_at(&list, 2);
    let style2_ref = style2_atom.borrow();
    let style2 = style2_ref.as_style().expect("MTMathStyle");
    assert_eq!(style2_ref.type_, T::Style, "{desc}");
    assert_eq!(style2_ref.nucleus, "", "{desc}");
    assert_eq!(style2.style, MTLineStyle::Script);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\textstyle y\\scriptstyle x", "{desc}");
}

#[test]
fn test_matrix() {
    let str = "\\begin{matrix} x & y \\\\ z & w \\end{matrix}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();

    assert_eq!(count(&list), 1);
    let table_atom = atom_at(&list, 0);
    let table_ref = table_atom.borrow();
    let table = table_ref.as_table().expect("MTMathTable");
    assert_eq!(table_ref.type_, T::Table);
    assert_eq!(table_ref.nucleus, "");
    assert_eq!(table.environment, "matrix");
    assert_eq!(table.inter_row_additional_spacing, 0.0);
    assert_eq!(table.inter_column_spacing, 18.0);
    assert_eq!(table.num_rows(), 2);
    assert_eq!(table.num_columns(), 2);

    for i in 0..2 {
        let alignment = table.get_alignment_for_column(i);
        assert_eq!(alignment, MTColumnAlignment::Center);
        for j in 0..2 {
            let cell = &table.cells[j][i];
            assert_eq!(count(cell), 2);
            let style_atom = atom_at(cell, 0);
            let style_ref = style_atom.borrow();
            let style = style_ref.as_style().expect("MTMathStyle");
            assert_eq!(style_ref.type_, T::Style);
            assert_eq!(style.style, MTLineStyle::Text);

            let atom = atom_at(cell, 1);
            assert_eq!(atom.borrow().type_, T::Variable);
        }
    }

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\begin{matrix}x&y\\\\ z&w\\end{matrix}");
}

#[test]
fn test_p_matrix() {
    let str = "\\begin{pmatrix} x & y \\\\ z & w \\end{pmatrix}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();

    assert_eq!(count(&list), 1);
    let inner_atom = atom_at(&list, 0);
    let inner_ref = inner_atom.borrow();
    let inner = inner_ref.as_inner().expect("MTInner");
    assert_eq!(inner_ref.type_, T::Inner, "{str}");
    assert_eq!(inner_ref.nucleus, "", "{str}");

    let inner_list = inner.inner_list.clone().unwrap();

    assert!(inner.left_boundary().is_some(), "{str}");
    assert_eq!(
        inner.left_boundary().unwrap().borrow().type_,
        T::Boundary,
        "{str}"
    );
    assert_eq!(
        inner.left_boundary().unwrap().borrow().nucleus,
        "(",
        "{str}"
    );

    assert!(inner.right_boundary().is_some(), "{str}");
    assert_eq!(
        inner.right_boundary().unwrap().borrow().type_,
        T::Boundary,
        "{str}"
    );
    assert_eq!(
        inner.right_boundary().unwrap().borrow().nucleus,
        ")",
        "{str}"
    );

    assert_eq!(count(&inner_list), 1);
    let table_atom = atom_at(&inner_list, 0);
    let table_ref = table_atom.borrow();
    let table = table_ref.as_table().expect("MTMathTable");
    assert_eq!(table_ref.type_, T::Table);
    assert_eq!(table_ref.nucleus, "");
    assert_eq!(table.environment, "matrix");
    assert_eq!(table.inter_row_additional_spacing, 0.0);
    assert_eq!(table.inter_column_spacing, 18.0);
    assert_eq!(table.num_rows(), 2);
    assert_eq!(table.num_columns(), 2);

    for i in 0..2 {
        let alignment = table.get_alignment_for_column(i);
        assert_eq!(alignment, MTColumnAlignment::Center);
        for j in 0..2 {
            let cell = &table.cells[j][i];
            assert_eq!(count(cell), 2);
            let style_atom = atom_at(cell, 0);
            let style_ref = style_atom.borrow();
            let style = style_ref.as_style().expect("MTMathStyle");
            assert_eq!(style_ref.type_, T::Style);
            assert_eq!(style.style, MTLineStyle::Text);

            let atom = atom_at(cell, 1);
            assert_eq!(atom.borrow().type_, T::Variable);
        }
    }

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(
        latex,
        "\\left( \\begin{matrix}x&y\\\\ z&w\\end{matrix}\\right) "
    );
}

#[test]
fn test_default_table() {
    let str = "x \\\\ y";
    let list = MTMathListBuilder::build_from_string(str).unwrap();

    assert_eq!(count(&list), 1);
    let table_atom = atom_at(&list, 0);
    let table_ref = table_atom.borrow();
    let table = table_ref.as_table().expect("MTMathTable");
    assert_eq!(table_ref.type_, T::Table);
    assert_eq!(table_ref.nucleus, "");
    assert!(table.environment.is_empty());
    assert_eq!(table.inter_row_additional_spacing, 1.0);
    assert_eq!(table.inter_column_spacing, 0.0);
    assert_eq!(table.num_rows(), 2);
    assert_eq!(table.num_columns(), 1);

    for i in 0..1 {
        let alignment = table.get_alignment_for_column(i);
        assert_eq!(alignment, MTColumnAlignment::Left);
        for j in 0..2 {
            let cell = &table.cells[j][i];
            assert_eq!(count(cell), 1);
            let atom = atom_at(cell, 0);
            assert_eq!(atom.borrow().type_, T::Variable);
        }
    }

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "x\\\\ y");
}

#[test]
fn test_default_table_with_cols() {
    let str = "x & y \\\\ z & w";
    let list = MTMathListBuilder::build_from_string(str).unwrap();

    assert_eq!(count(&list), 1);
    let table_atom = atom_at(&list, 0);
    let table_ref = table_atom.borrow();
    let table = table_ref.as_table().expect("MTMathTable");
    assert_eq!(table_ref.type_, T::Table);
    assert_eq!(table_ref.nucleus, "");
    assert!(table.environment.is_empty());
    assert_eq!(table.inter_row_additional_spacing, 1.0);
    assert_eq!(table.inter_column_spacing, 0.0);
    assert_eq!(table.num_rows(), 2);
    assert_eq!(table.num_columns(), 2);

    for i in 0..2 {
        let alignment = table.get_alignment_for_column(i);
        assert_eq!(alignment, MTColumnAlignment::Left);
        for j in 0..2 {
            let cell = &table.cells[j][i];
            assert_eq!(count(cell), 1);
            let atom = atom_at(cell, 0);
            assert_eq!(atom.borrow().type_, T::Variable);
        }
    }

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "x&y\\\\ z&w");
}

#[test]
fn test_eqalign() {
    let str1 = "\\begin{eqalign}x&y\\\\ z&w\\end{eqalign}";
    let str2 = "\\begin{split}x&y\\\\ z&w\\end{split}";
    let str3 = "\\begin{aligned}x&y\\\\ z&w\\end{aligned}";
    for str in [str1, str2, str3] {
        let list = MTMathListBuilder::build_from_string(str).unwrap();

        assert_eq!(count(&list), 1);
        let table_atom = atom_at(&list, 0);
        let table_ref = table_atom.borrow();
        let table = table_ref.as_table().expect("MTMathTable");
        assert_eq!(table_ref.type_, T::Table);
        assert_eq!(table_ref.nucleus, "");
        assert_eq!(table.inter_row_additional_spacing, 1.0);
        assert_eq!(table.inter_column_spacing, 0.0);
        assert_eq!(table.num_rows(), 2);
        assert_eq!(table.num_columns(), 2);

        for i in 0..2 {
            let alignment = table.get_alignment_for_column(i);
            assert_eq!(
                alignment,
                if i == 0 {
                    MTColumnAlignment::Right
                } else {
                    MTColumnAlignment::Left
                }
            );
            for j in 0..2 {
                let cell = &table.cells[j][i];
                if i == 0 {
                    assert_eq!(count(cell), 1);
                    let atom = atom_at(cell, 0);
                    assert_eq!(atom.borrow().type_, T::Variable);
                } else {
                    assert_eq!(count(cell), 2);
                    check_atom_types(Some(cell), &[T::Ordinary, T::Variable], str);
                }
            }
        }

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(Some(&list));
        assert_eq!(latex, str);
    }
}

#[test]
fn test_display_lines() {
    let str1 = "\\begin{displaylines}x\\\\ y\\end{displaylines}";
    let str2 = "\\begin{gather}x\\\\ y\\end{gather}";
    for str in [str1, str2] {
        let list = MTMathListBuilder::build_from_string(str);

        assert!(list.is_some());
        assert_eq!(list.as_ref().map(count), Some(1));
        let table_atom = atom_at(list.as_ref().unwrap(), 0);
        let table_ref = table_atom.borrow();
        let table = table_ref.as_table().expect("MTMathTable");
        assert_eq!(table_ref.type_, T::Table);
        assert_eq!(table_ref.nucleus, "");
        assert_eq!(table.inter_row_additional_spacing, 1.0);
        assert_eq!(table.inter_column_spacing, 0.0);
        assert_eq!(table.num_rows(), 2);
        assert_eq!(table.num_columns(), 1);

        for i in 0..1 {
            let alignment = table.get_alignment_for_column(i);
            assert_eq!(alignment, MTColumnAlignment::Center);
            for j in 0..2 {
                let cell = &table.cells[j][i];
                assert_eq!(count(cell), 1);
                let atom = atom_at(cell, 0);
                assert_eq!(atom.borrow().type_, T::Variable);
            }
        }

        // convert it back to latex
        let latex = MTMathListBuilder::math_list_to_string(list.as_ref());
        assert_eq!(latex, str);
    }
}

#[test]
fn test_errors() {
    let data = get_test_data_parse_errors();
    for test_case in &data {
        let str = test_case.0;
        let mut error: Option<MTParseError> = None;
        let list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
        let desc = format!("Error for string:{str}");
        assert!(list.is_none(), "{desc}");
        assert!(error.is_some(), "{desc}");
        let error = error.unwrap();
        assert_eq!(error.domain(), MT_PARSE_ERROR, "{desc}");
        let num = test_case.1;
        assert_eq!(error.code_value(), num as i32, "{desc}");
    }
}

#[test]
fn test_custom() {
    let str = "\\lcm(a,b)";
    let mut error: Option<MTParseError> = None;
    let mut list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
    assert!(list.is_none());
    assert!(error.is_some());

    MTMathAtomFactory::add_latex_symbol(
        "lcm",
        &MTMathAtomFactory::operator_with_name("lcm", false).borrow(),
    );
    error = None;
    list = MTMathListBuilder::build_from_string_with_error(str, &mut error);
    let atom_types = [
        T::LargeOperator,
        T::Open,
        T::Variable,
        T::Punctuation,
        T::Variable,
        T::Close,
    ];
    check_atom_types(list.as_ref(), &atom_types, "Error for lcm");

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(list.as_ref());
    assert_eq!(latex, "\\lcm (a,b)");
}

#[test]
fn test_font_single() {
    let str = "\\mathbf x";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let atom = atom_at(&list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "x", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Bold);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\mathbf{x}", "{desc}");
}

#[test]
fn test_font_one_char() {
    let str = "\\cal xy";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 2, "{desc}");
    let mut atom = atom_at(&list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "x", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Caligraphic);

    atom = atom_at(&list, 1);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "y", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::DefaultStyle);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\mathcal{x}y", "{desc}");
}

#[test]
fn test_font_multiple_chars() {
    let str = "\\frak{xy}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 2, "{desc}");
    let mut atom = atom_at(&list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "x", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Fraktur);

    atom = atom_at(&list, 1);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "y", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Fraktur);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\mathfrak{xy}", "{desc}");
}

#[test]
fn test_font_one_char_inside() {
    let str = "\\sqrt \\mathrm x y";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 2, "{desc}");

    let rad_atom = atom_at(&list, 0);
    let rad_ref = rad_atom.borrow();
    let rad = rad_ref.as_radical().expect("MTRadical");
    assert_eq!(rad_ref.type_, T::Radical, "{desc}");
    assert_eq!(rad_ref.nucleus, "", "{desc}");

    let sub_list = rad.radicand.clone().unwrap();
    let mut atom = atom_at(&sub_list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "x", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Roman);

    atom = atom_at(&list, 1);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "y", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::DefaultStyle);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sqrt{\\mathrm{x}}y", "{desc}");
}

#[test]
fn test_text() {
    let str = "\\text{x y}";
    let list = MTMathListBuilder::build_from_string(str).unwrap();
    let desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 3, "{desc}");
    let mut atom = atom_at(&list, 0);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "x", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Roman);

    atom = atom_at(&list, 1);
    assert_eq!(atom.borrow().type_, T::Ordinary, "{desc}");
    assert_eq!(atom.borrow().nucleus, " ", "{desc}");

    atom = atom_at(&list, 2);
    assert_eq!(atom.borrow().type_, T::Variable, "{desc}");
    assert_eq!(atom.borrow().nucleus, "y", "{desc}");
    assert_eq!(atom.borrow().font_style, MTFontStyle::Roman);

    // convert it back to latex
    let latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\mathrm{x\\  y}", "{desc}");
}

#[test]
fn test_limits() {
    // Int with no limits (default)
    let mut str = "\\int";
    let mut list = MTMathListBuilder::build_from_string(str).unwrap();
    let mut desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let mut op = atom_at(&list, 0);
    assert!(op.borrow().as_large_operator().is_some(), "MTLargeOperator");
    assert_eq!(op.borrow().type_, T::LargeOperator, "{desc}");
    assert!(!op.borrow().as_large_operator().unwrap().limits);

    // convert it back to latex
    let mut latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\int ", "{desc}");

    // Int with limits
    str = "\\int\\limits";
    list = MTMathListBuilder::build_from_string(str).unwrap();
    desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    op = atom_at(&list, 0);
    assert!(op.borrow().as_large_operator().is_some(), "MTLargeOperator");
    assert_eq!(op.borrow().type_, T::LargeOperator, "{desc}");
    assert!(op.borrow().as_large_operator().unwrap().limits);

    // convert it back to latex
    latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\int \\limits ", "{desc}");
}

#[test]
fn test_no_limits() {
    // Sum with limits (default)
    let mut str = "\\sum";
    let mut list = MTMathListBuilder::build_from_string(str).unwrap();
    let mut desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    let mut op = atom_at(&list, 0);
    assert!(op.borrow().as_large_operator().is_some(), "MTLargeOperator");
    assert_eq!(op.borrow().type_, T::LargeOperator, "{desc}");
    assert!(op.borrow().as_large_operator().unwrap().limits);

    // convert it back to latex
    let mut latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sum ", "{desc}");

    // Int with limits
    str = "\\sum\\nolimits";
    list = MTMathListBuilder::build_from_string(str).unwrap();
    desc = format!("Error for string:{str}");

    assert_eq!(count(&list), 1, "{desc}");
    op = atom_at(&list, 0);
    assert!(op.borrow().as_large_operator().is_some(), "MTLargeOperator");
    assert_eq!(op.borrow().type_, T::LargeOperator, "{desc}");
    assert!(!op.borrow().as_large_operator().unwrap().limits);

    // convert it back to latex
    latex = MTMathListBuilder::math_list_to_string(Some(&list));
    assert_eq!(latex, "\\sum \\nolimits ", "{desc}");
}
