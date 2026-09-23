//! Port of `Tests/DownrightAppTests/ReviewSidecarTests.swift` (all tests).

use upleft_app::review::review_anchor_resolver::{CONTEXT_LENGTH, ReviewAnchorResolver};
use upleft_app::review::review_sidecar::{
    LocalReviewSidecarStore, ReviewAnchor, ReviewAnchorStatus, ReviewApplyResult, ReviewItem, ReviewKind,
    ReviewSidecar, ReviewSidecarEngine, ReviewSidecarError, ReviewSidecarStore, ReviewState,
};
use upleft_core::contracts::Uuid;
use upleft_core::ns_range::NSRange;
use upleft_foundation::url::FileUrl;

/// `(text as NSString).range(of: needle)` for the ASCII texts used here.
fn range_of(text: &str, needle: &str) -> NSRange {
    let start = text.find(needle).unwrap();
    NSRange::new(text[..start].encode_utf16().count() as isize, needle.encode_utf16().count() as isize)
}

fn temporary_directory() -> FileUrl {
    let path = std::env::temp_dir().join(Uuid::new_v4().hyphenated().to_string().to_uppercase());
    std::fs::create_dir_all(&path).unwrap();
    FileUrl::from_path_is_directory(path.to_str().unwrap(), true)
}

#[test]
fn anchor_resolves_exact_and_far_shifted_source() {
    let before = "prefix\n".repeat(20) + "Keep this sentence." + "\ntrailer";
    let range = range_of(&before, "Keep this sentence.");
    let anchor = ReviewAnchorResolver::make_anchor(&before, range, CONTEXT_LENGTH).unwrap();
    assert_eq!(ReviewAnchorResolver::resolve(&anchor, &before, CONTEXT_LENGTH).status, ReviewAnchorStatus::Exact);

    let after = "new line\n".repeat(20) + &before;
    let shifted = ReviewAnchorResolver::resolve(&anchor, &after, CONTEXT_LENGTH);
    assert_eq!(shifted.status, ReviewAnchorStatus::Shifted);
    assert_eq!(shifted.range.map(|range| range.location), Some(range.location + 9 * 20));
}

#[test]
fn changed_context_is_stale_and_missing_source_is_orphan() {
    let text = "before\nKeep this\nafter";
    let range = range_of(text, "Keep this");
    let anchor = ReviewAnchorResolver::make_anchor(text, range, CONTEXT_LENGTH).unwrap();
    assert_eq!(
        ReviewAnchorResolver::resolve(&anchor, "changed\nKeep this\nafter", CONTEXT_LENGTH).status,
        ReviewAnchorStatus::Stale
    );
    assert_eq!(ReviewAnchorResolver::resolve(&anchor, "before\nGone\nafter", CONTEXT_LENGTH).status, ReviewAnchorStatus::Orphan);
}

#[test]
fn suggestion_preview_requires_fresh_source_and_returns_one_edit() {
    let text = "Use the old API.";
    let range = range_of(text, "old API");
    let anchor = ReviewAnchorResolver::make_anchor(text, range, CONTEXT_LENGTH).unwrap();
    let review = ReviewItem::new(ReviewKind::Suggestion, anchor, "Use new API", Some("new API"), ReviewState::Open);
    let ReviewApplyResult::Applied(edit) = ReviewSidecarEngine::apply_suggestion(&review, text) else {
        panic!("fresh suggestion should produce an edit");
    };
    assert_eq!(edit.range, range);
    assert_eq!(edit.replacement, "new API");
    assert!(
        matches!(ReviewSidecarEngine::apply_suggestion(&review, "Use another API."), ReviewApplyResult::Stale(_)),
        "changed source must reject the suggestion"
    );
}

#[test]
fn review_creation_keeps_comments_and_suggestions_separate() {
    let text = "Review this line.";
    let range = range_of(text, "this line");
    let comment = ReviewSidecarEngine::make_review(ReviewKind::Comment, text, range, "Clarify this.", None).unwrap();
    let suggestion =
        ReviewSidecarEngine::make_review(ReviewKind::Suggestion, text, range, "Use a shorter phrase.", Some("the line"))
            .unwrap();
    assert_eq!(comment.kind, ReviewKind::Comment);
    assert_eq!(comment.replacement, None);
    assert_eq!(suggestion.kind, ReviewKind::Suggestion);
    assert_eq!(suggestion.replacement.as_deref(), Some("the line"));
}

#[test]
fn sidecar_store_round_trips_outside_markdown_file() {
    let directory = temporary_directory();
    let document = directory.appending_path_component("note.md");
    let anchor = ReviewAnchorResolver::make_anchor("Text", NSRange::new(0, 4), CONTEXT_LENGTH).unwrap();
    let original = ReviewSidecar {
        version: 1,
        reviews: vec![
            ReviewItem::new(ReviewKind::Comment, anchor.clone(), "Check this", None, ReviewState::Open),
            ReviewItem::new(ReviewKind::Suggestion, anchor, "Replace this", Some("Other"), ReviewState::Rejected),
        ],
    };
    let store = LocalReviewSidecarStore::new();
    store.save(&original, &document).unwrap();
    let loaded = store.load(&document).unwrap();
    assert_eq!(loaded, original);
    assert!(std::path::Path::new(&LocalReviewSidecarStore::sidecar_url(&document).path()).exists());
    let _ = std::fs::remove_dir_all(directory.path());
}

#[test]
fn sidecar_store_rejects_oversized_repository_input_before_decoding() {
    let directory = temporary_directory();
    let document = directory.appending_path_component("note.md");
    let sidecar = LocalReviewSidecarStore::sidecar_url(&document);
    std::fs::write(sidecar.path(), vec![0x20u8; LocalReviewSidecarStore::MAXIMUM_BYTES + 1]).unwrap();
    assert_eq!(LocalReviewSidecarStore::new().load(&document), Err(ReviewSidecarError::FileReadTooLarge));
    let _ = std::fs::remove_dir_all(directory.path());
}

#[test]
fn hostile_anchor_integers_cannot_overflow_or_escape_returned_ranges() {
    let anchor = ReviewAnchor {
        range: NSRange::new(isize::MAX, isize::MAX),
        selected_text: "Text".into(),
        before_fingerprint: String::new(),
        after_fingerprint: String::new(),
    };
    let result = ReviewAnchorResolver::resolve(&anchor, "Text", CONTEXT_LENGTH);
    assert_eq!(result.range, None);
    assert_eq!(result.status, ReviewAnchorStatus::Stale);
}

#[test]
fn critic_markup_is_detected_but_never_generated() {
    let source = "++add++ and {--remove--} and ==mark==";
    assert_eq!(ReviewSidecarEngine::critic_markup_ranges(source).len(), 3);
    assert!(ReviewSidecarEngine::critic_markup_ranges("plain markdown").is_empty());
}
