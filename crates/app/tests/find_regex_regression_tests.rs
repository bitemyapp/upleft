//! Port of `Tests/DownrightAppTests/FindRegexRegressionTests.swift`
//! (`FindRegexRegressionTests` and `FindSessionReplacementTests`).

use upleft_app::support::find_engine::{FindEngine, FindQuery, FindSession};
use upleft_core::NSRange;

fn regex(text: &str) -> FindQuery {
    FindQuery::regex(text)
}

// MARK: - FindRegexRegressionTests

#[test]
fn replacement_preserves_lookaround_captures() {
    let text = "foo bar foo baz";
    let query = regex(r"(foo)(?= (bar|baz))");
    let matches = FindEngine::matches(text, &query);
    assert_eq!(matches.len(), 2);
    let first = *matches.first().expect("a match");
    assert_eq!(FindEngine::replacement(first, text, &query, "$2:$1"), "bar:foo");
    let edits = FindEngine::replace_all_edits(text, &query, "$2:$1");
    assert_eq!(edits.iter().map(|edit| edit.replacement.as_str()).collect::<Vec<_>>(), ["bar:foo", "baz:foo"]);
    assert_eq!(edits.iter().map(|edit| edit.range).collect::<Vec<_>>(), matches);
}

#[test]
fn replacement_keeps_original_anchor_semantics() {
    let text = "foo bar";
    let query = regex(r"(foo$)|(foo)");
    let found = *FindEngine::matches(text, &query).first().expect("a match");
    assert_eq!(FindEngine::replacement(found, text, &query, "$1/$2"), "/foo");
    assert_eq!(FindEngine::replace_all_edits(text, &query, "$1/$2").first().map(|e| e.replacement.as_str()), Some("/foo"));
}

#[test]
fn replacement_preserves_selection_scope() {
    let text = "outside foo bar outside";
    let query = FindQuery { scope: Some(NSRange::new(8, 7)), ..regex(r"^(foo)(?= bar$)") };
    let found = *FindEngine::matches(text, &query).first().expect("a match");
    assert_eq!(FindEngine::replacement(found, text, &query, "$1!"), "foo!");
    assert_eq!(FindEngine::replace_all_edits(text, &query, "$1!").first().map(|e| e.replacement.as_str()), Some("foo!"));
}

#[test]
fn whole_word_applies_to_every_alternative_without_shifting_captures() {
    let text = "foobar foo bar";
    let query = FindQuery { whole_word: true, ..regex(r"(foo)|(bar)") };
    assert_eq!(FindEngine::matches(text, &query), vec![NSRange::new(7, 3), NSRange::new(11, 3)]);
    assert_eq!(
        FindEngine::replace_all_edits(text, &query, "$1/$2").iter().map(|e| e.replacement.as_str()).collect::<Vec<_>>(),
        ["foo/", "/bar"]
    );
}

#[test]
fn literal_replacement_leaves_dollar_signs_and_backslashes_untouched() {
    let query = FindQuery::new("foo");
    assert_eq!(
        FindEngine::replace_all_edits("foo foo", &query, r"$1\bar").iter().map(|e| e.replacement.as_str()).collect::<Vec<_>>(),
        [r"$1\bar", r"$1\bar"]
    );
}

// MARK: - FindSessionReplacementTests

#[test]
fn selected_replacement_uses_original_lookaround_captures() {
    let mut session = FindSession::new();
    let text = "foo bar foo baz";
    session.update(regex(r"(foo)(?= (bar|baz))"), text, 0);
    let first = session.replacement_edit(text, "$2:$1", 0).expect("an edit");
    assert_eq!(first.range, NSRange::new(0, 3));
    assert_eq!(first.replacement, "bar:foo");
    session.advance(true);
    let second = session.replacement_edit(text, "$2:$1", 0).expect("an edit");
    assert_eq!(second.range, NSRange::new(8, 3));
    assert_eq!(second.replacement, "baz:foo");
}

#[test]
fn selected_replacement_preserves_anchored_selection_scope() {
    let mut session = FindSession::new();
    let text = "outside foo bar outside";
    let query = FindQuery { scope: Some(NSRange::new(8, 7)), ..regex(r"^(foo)(?= bar$)") };
    session.update(query, text, 0);
    let edit = session.replacement_edit(text, "$1!", 0).expect("an edit");
    assert_eq!(edit.range, NSRange::new(8, 3));
    assert_eq!(edit.replacement, "foo!");
}

#[test]
fn same_length_edit_refreshes_captures_before_replacement() {
    let mut session = FindSession::new();
    session.update(regex(r"(foo)(?= (bar|baz))"), "foo bar", 0);
    let edit = session.replacement_edit("foo baz", "$2:$1", 0).expect("an edit");
    assert_eq!(edit.range, NSRange::new(0, 3));
    assert_eq!(edit.replacement, "baz:foo");
}

#[test]
fn deleting_prefix_refreshes_from_the_live_caret_rather_than_the_old_match_offset() {
    let mut session = FindSession::new();
    session.update(FindQuery::new("foo"), "old foo tail foo", 0);
    assert_eq!(session.current_match().map(|range| range.location), Some(4));
    let edit = session.replacement_edit("foo tail foo", "new", 0).expect("an edit");
    assert_eq!(edit.range, NSRange::new(0, 3));
    assert_eq!(edit.replacement, "new");
}

#[test]
fn canonical_unicode_equality_does_not_reuse_stale_source_ranges() {
    let mut session = FindSession::new();
    let original = "[\u{e9}] tail";
    let edited = "[e\u{301}] tail";
    assert!(upleft_swift_text::str_eq(original, edited));
    session.update(regex(r"\[(.*?)\]"), original, 0);
    let edit = session.replacement_edit(edited, "$1", 0).expect("an edit");
    assert_eq!(edit.range, NSRange::new(0, 4));
    assert_eq!(edit.replacement.encode_utf16().collect::<Vec<_>>(), "e\u{301}".encode_utf16().collect::<Vec<_>>());
}

#[test]
fn removed_match_and_cleared_session_produce_no_replacement() {
    let mut session = FindSession::new();
    session.update(FindQuery::new("foo"), "foo", 0);
    assert!(session.replacement_edit("bar", "new", 0).is_none());
    assert!(session.matches().is_empty());
    session.update(FindQuery::new("foo"), "foo", 0);
    session.clear();
    assert!(session.replacement_edit("foo", "new", 0).is_none());
}

#[test]
fn changing_query_replaces_cached_matches_and_keeps_literal_template() {
    let mut session = FindSession::new();
    session.update(regex("foo"), "foo bar", 0);
    session.update(FindQuery::new("bar"), "foo bar", 0);
    let edit = session.replacement_edit("foo bar", r"$1\tail", 0).expect("an edit");
    assert_eq!(edit.range, NSRange::new(4, 3));
    assert_eq!(edit.replacement, r"$1\tail");
}
