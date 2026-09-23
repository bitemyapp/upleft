//! Port of `fuzzyMatcherFindsSubsequence` from
//! `Tests/DownrightAppTests/OptimizationRegressionTests.swift` (the rest of
//! that file belongs to other ports).

use upleft_app::panels::fuzzy_matcher::FuzzyMatcher;

#[test]
fn fuzzy_matcher_finds_subsequence() {
    let found = FuzzyMatcher::r#match("dpl", "Document pipeline");
    assert!(found.is_some());
    assert!(FuzzyMatcher::r#match("zzz", "Document pipeline").is_none());
}
