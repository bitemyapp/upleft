//! Port of `ListOrnamentTests.swift`: every nesting level draws its own
//! ornament, and completion styling stops at the item. Asserted structurally:
//! an ornament that resolves to its own block is one that draws, and
//! `isFirstParagraphOfBlock` is the exact predicate `drawObject` gates on.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSStrikethroughStyleAttributeName;
use upleft_render::appkit_compat::attribute_value;
use upleft_render::fragments::fragment_base::DownrightFragment;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("list_ornament_nested_bullets_draw", nested_bullets_draw),
    ("list_ornament_nested_checkboxes_draw", nested_checkboxes_draw),
    ("list_ornament_nested_ordered_draws", nested_ordered_draws),
    ("list_ornament_bullet_under_task_draws", bullet_under_task_draws),
    ("list_ornament_bullet_inside_block_quote_draws", bullet_inside_block_quote_draws),
    ("list_ornament_continuation_paragraph_draws_nothing", continuation_paragraph_draws_nothing),
    ("list_ornament_completed_task_is_struck", completed_task_is_struck),
    ("list_ornament_completion_does_not_bleed_into_subtree", completion_does_not_bleed_into_subtree),
    ("list_ornament_open_parent_does_not_strike_subtree", open_parent_does_not_strike_subtree),
    ("list_ornament_completion_covers_the_items_own_blocks", completion_covers_the_items_own_blocks),
];

/// The ornaments that actually put a mark on screen.
fn drawn_ornaments(container: &MarkdownContainerView) -> Vec<Retained<DownrightFragment>> {
    fragments_of_class(container.text_view(), "ListOrnamentFragment")
        .into_iter()
        .filter(|fragment| fragment.is_first_paragraph_of_block())
        .collect()
}

fn details(container: &MarkdownContainerView) -> Vec<String> {
    drawn_ornaments(container).iter().map(|fragment| fragment.payload().detail().to_owned()).collect()
}

fn nested_bullets_draw(mtm: MainThreadMarker) {
    let container = read_container("- level one\n  - level two\n    - level three", mtm);
    let drawn = drawn_ornaments(&container);
    expect!(drawn.len() == 3);
    let locations: std::collections::HashSet<isize> =
        drawn.iter().map(|fragment| fragment.payload().source_range().location).collect();
    expect!(locations.len() == 3);
    expect!(drawn.iter().all(|fragment| fragment.payload().detail().starts_with("unordered:")));
}

fn nested_checkboxes_draw(mtm: MainThreadMarker) {
    let container = read_container("- [x] checked parent\n  - [ ] nested open\n  - [x] nested done", mtm);
    expect!(details(&container) == ["task:checked", "task:unchecked", "task:checked"]);
}

fn nested_ordered_draws(mtm: MainThreadMarker) {
    let container = read_container("1. first\n   1. nested first\n   2. nested second", mtm);
    expect!(details(&container) == ["ordered:1", "ordered:1", "ordered:2"]);
}

fn bullet_under_task_draws(mtm: MainThreadMarker) {
    let container = read_container("- [ ] task parent\n  - plain nested bullet", mtm);
    expect!(details(&container) == ["task:unchecked", "unordered:2"]);
}

fn bullet_inside_block_quote_draws(mtm: MainThreadMarker) {
    let container = read_container("> - quoted one\n>   - quoted two", mtm);
    expect!(drawn_ornaments(&container).len() == 2);
}

/// A continuation paragraph belongs to the item but begins no block of its
/// own, so it must not acquire a second bullet.
fn continuation_paragraph_draws_nothing(mtm: MainThreadMarker) {
    let container = read_container("- item with two paragraphs\n\n  the second paragraph\n\n- next item", mtm);
    expect!(drawn_ornaments(&container).len() == 2);
}

fn is_struck_through(container: &MarkdownContainerView, needle: &str) -> bool {
    let storage = unsafe { container.text_view().textStorage() }.expect("storage");
    let text = storage_string(&storage);
    let range = range_of(&text, needle);
    assert!(range.location != upleft_core::ns_range::NS_NOT_FOUND, "'{needle}' is not in the document");
    // SAFETY: AppKit exports the key as an immutable global.
    attribute_value(&storage, unsafe { NSStrikethroughStyleAttributeName }, range.location as usize).is_some()
}

fn completed_task_is_struck(mtm: MainThreadMarker) {
    let container = read_container("- [x] this one is done", mtm);
    expect!(is_struck_through(&container, "this one is done"));
}

fn completion_does_not_bleed_into_subtree(mtm: MainThreadMarker) {
    let container = read_container("- [x] checked parent\n  - [ ] nested open\n  - [x] nested done", mtm);
    expect!(is_struck_through(&container, "checked parent"));
    expect!(!is_struck_through(&container, "nested open"));
    expect!(is_struck_through(&container, "nested done"));
}

fn open_parent_does_not_strike_subtree(mtm: MainThreadMarker) {
    let container = read_container("- [ ] open parent\n  - [ ] nested open\n  - [x] nested done", mtm);
    expect!(!is_struck_through(&container, "open parent"));
    expect!(!is_struck_through(&container, "nested open"));
    expect!(is_struck_through(&container, "nested done"));
}

fn completion_covers_the_items_own_blocks(mtm: MainThreadMarker) {
    let container = read_container("- [x] done item\n\n  still the same item\n\n- [ ] other", mtm);
    expect!(is_struck_through(&container, "done item"));
    expect!(is_struck_through(&container, "still the same item"));
    expect!(!is_struck_through(&container, "other"));
}
