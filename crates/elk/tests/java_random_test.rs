//! Port of `Tests/ElkSwiftTests/JavaRandomTest.swift`: the `Random` class
//! matches `java.util.Random` (reference values from Java 17, `new Random(1)`).

use upleft_elk::org::eclipse::elk::alg::layered::graph_configurator::Random;

#[test]
fn test_next_int_sequence() {
    let expected: [i64; 10] = [-1155869325, 431529176, 1761283695, 1749940626, 892128508, 155629808, 1429008869, -1465154083, -138487339, -1242363800];
    let mut r = Random::with_seed(1);
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(r.next_int(), exp, "nextInt() call #{}", i + 1);
    }
}

#[test]
fn test_next_double_sequence() {
    let expected: [f64; 5] = [0.7308781907032909, 0.41008081149220166, 0.20771484130971707, 0.3327170559595112, 0.9677559094241207];
    let mut r = Random::with_seed(1);
    for (i, &exp) in expected.iter().enumerate() {
        let got = r.next_double();
        assert!((got - exp).abs() <= 1e-15, "nextDouble() call #{}: {got} != {exp}", i + 1);
    }
}

#[test]
fn test_next_boolean_derived_from_next_int() {
    let int_values: [i64; 10] = [-1155869325, 431529176, 1761283695, 1749940626, 892128508, 155629808, 1429008869, -1465154083, -138487339, -1242363800];
    let mut r = Random::with_seed(1);
    for (i, &v) in int_values.iter().enumerate() {
        assert_eq!(r.next_boolean(), v < 0, "nextBoolean() call #{}", i + 1);
    }
}

#[test]
fn test_next_float_derived_from_next_int() {
    let int_values: [i32; 5] = [-1155869325, 431529176, 1761283695, 1749940626, 892128508];
    let mut r = Random::with_seed(1);
    for (i, &v) in int_values.iter().enumerate() {
        let exp = ((v as u32) >> 8) as f32 / (1u32 << 24) as f32;
        let got = r.next_float();
        assert!((got - exp).abs() <= 1e-9, "nextFloat() call #{}", i + 1);
    }
}

#[test]
fn test_next_int_bounded() {
    let expected = [5, 8, 7, 3, 4, 4, 4, 6, 8, 8];
    let mut r = Random::with_seed(1);
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(r.next_int_bounded(10), exp, "nextInt(10) call #{}", i + 1);
    }
}

#[test]
fn test_seed0() {
    let mut r = Random::with_seed(0);
    assert_eq!(r.next_int(), -1155484576);
}

#[test]
fn test_set_seed_resets() {
    let mut r = Random::with_seed(42);
    let _ = r.next_int();
    r.set_seed(1);
    assert_eq!(r.next_int(), -1155869325);
}
