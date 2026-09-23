//! Port of `Tests/ElkSwiftTests/BinaryIndexedTreeTests.swift`.

use upleft_elk::org::eclipse::elk::alg::layered::p3order::counting::binary_indexed_tree::BinaryIndexedTree;

#[test]
fn test_sum_before() {
    let mut tree = BinaryIndexedTree::new(5);
    tree.add(1);
    tree.add(2);
    tree.add(1);
    assert_eq!(tree.rank(1), 0);
    assert_eq!(tree.rank(2), 2);
}

#[test]
fn test_size() {
    let mut tree = BinaryIndexedTree::new(5);
    tree.add(2);
    tree.add(1);
    tree.add(1);
    assert_eq!(tree.size(), 3);
}

#[test]
fn test_remove_all() {
    let mut tree = BinaryIndexedTree::new(5);
    tree.add(0);
    tree.add(2);
    tree.add(1);
    tree.add(1);
    tree.remove_all(1);
    assert_eq!(tree.size(), 2);
    assert_eq!(tree.rank(2), 1);
    // Idempotent
    tree.remove_all(1);
    assert_eq!(tree.size(), 2);
    assert_eq!(tree.rank(2), 1);
}
