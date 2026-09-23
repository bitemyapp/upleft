//! Port of elk-swift's `Tests/ElkSwiftTests/JavaRandomTest.swift`: the
//! `java.util.Random` replica against reference values from Java 17
//! (`new Random(1)`).

use upleft_elk::org::eclipse::elk::alg::layered::graph_configurator::Random;

const INTS: [i64; 10] =
    [-1155869325, 431529176, 1761283695, 1749940626, 892128508, 155629808, 1429008869, -1465154083, -138487339, -1242363800];

#[test]
fn next_int_sequence() {
    let mut r = Random::with_seed(1);
    for (i, &expected) in INTS.iter().enumerate() {
        assert_eq!(r.next_int(), expected, "nextInt() call #{}", i + 1);
    }
}

#[test]
fn next_double_sequence() {
    let expected = [0.7308781907032909, 0.41008081149220166, 0.20771484130971707, 0.3327170559595112, 0.9677559094241207];
    let mut r = Random::with_seed(1);
    for (i, &expected) in expected.iter().enumerate() {
        let got = r.next_double();
        assert!((got - expected).abs() <= 1e-15, "nextDouble() call #{}: {got} vs {expected}", i + 1);
    }
}

#[test]
fn next_boolean_derived_from_next_int() {
    // nextBoolean() = next(1) != 0, the sign bit of the matching nextInt().
    let mut r = Random::with_seed(1);
    for (i, &value) in INTS.iter().enumerate() {
        assert_eq!(r.next_boolean(), value < 0, "nextBoolean() call #{}", i + 1);
    }
}

#[test]
fn next_float_derived_from_next_int() {
    // nextFloat() = next(24) / (1 << 24), next(24) = nextInt() >>> 8.
    let mut r = Random::with_seed(1);
    for (i, &value) in INTS[..5].iter().enumerate() {
        let expected = ((value as i32 as u32) >> 8) as f32 / (1u32 << 24) as f32;
        let got = r.next_float();
        assert!((got - expected).abs() <= 1e-9, "nextFloat() call #{}: {got} vs {expected}", i + 1);
    }
}

#[test]
fn next_int_bounded() {
    let expected = [5, 8, 7, 3, 4, 4, 4, 6, 8, 8];
    let mut r = Random::with_seed(1);
    for (i, &expected) in expected.iter().enumerate() {
        assert_eq!(r.next_int_bounded(10), expected, "nextInt(10) call #{}", i + 1);
    }
}

#[test]
fn seed_0() {
    let mut r = Random::with_seed(0);
    assert_eq!(r.next_int(), -1155484576);
}

#[test]
fn set_seed_resets() {
    let mut r = Random::with_seed(42);
    let _ = r.next_int();
    r.set_seed(1);
    assert_eq!(r.next_int(), -1155869325);
}
