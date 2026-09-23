//! Port of `LayoutFillerTests.swift`: hidden-marker layout fillers carry the
//! storage's own attributes and keep source length.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSTextStorage};
use objc2_foundation::{NSAttributedString, NSMutableDictionary, NSString};
use upleft_render::engine::display_map::DisplaySubstitution;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("layout_fillers_match_storage_attributes", layout_fillers_match_storage_attributes),
    ("theme_swap_rebuilds_displayed_paragraph_attributes", theme_swap_rebuilds_displayed_paragraph_attributes),
    ("layout_fillers_preserve_source_length", layout_fillers_preserve_source_length),
    ("caret_move_leaves_other_paragraph_fillers_intact", caret_move_leaves_other_paragraph_fillers_intact),
];

const MARKER_RICH_TEXT: &str = "# Heading one\n\nSome **bold** text with `code` and *emphasis* on one line.\n\n## Heading two\n\n- [ ] A task with **bold** inside\n- [x] A finished task\n\n> A quote with `inline code`.\n\n### Heading three\n\nMore prose with [a link](https://example.com) and **more bold**.";

fn decorated_view(mtm: MainThreadMarker) -> (Retained<MarkdownTextView>, Retained<NSTextStorage>) {
    let (view, storage) = view_with(MARKER_RICH_TEXT, rect(0.0, 0.0, 720.0, 420.0), mtm);
    view.update(parse(MARKER_RICH_TEXT), &wholesale(), true);
    (view, storage)
}

/// The attributes that decide how a run is laid out and drawn.
fn rendering_attributes(attributed: &NSAttributedString, index: usize) -> Retained<NSMutableDictionary<NSString, AnyObject>> {
    let attributes = unsafe { attributed.attributesAtIndex_effectiveRange(index, std::ptr::null_mut()) };
    let mutable: Retained<NSMutableDictionary<NSString, AnyObject>> = unsafe { msg_send![&*attributes, mutableCopy] };
    for key in MarkdownTextView::private_attribute_keys() {
        mutable.removeObjectForKey(key);
    }
    mutable
}

fn filler_matches_storage(substitution: &DisplaySubstitution, storage: &NSTextStorage) -> bool {
    let Some(replacement) = substitution.replacement.as_ref().filter(|replacement| replacement.length() > 0) else {
        return true;
    };
    let actual = rendering_attributes(replacement, 0);
    let expected = rendering_attributes(storage, substitution.source_range.location as usize);
    actual.isEqualToDictionary(&expected)
}

fn hidden(view: &MarkdownTextView) -> Vec<DisplaySubstitution> {
    view.current_display_map().substitutions().into_iter().filter(|sub| sub.is_hidden).collect()
}

fn layout_fillers_match_storage_attributes(mtm: MainThreadMarker) {
    let (view, storage) = decorated_view(mtm);
    let hidden = hidden(&view);
    expect!(!hidden.is_empty());
    for substitution in hidden {
        expect!(
            filler_matches_storage(&substitution, &storage),
            "filler at {:?} lost the storage's attributes",
            substitution.source_range
        );
    }
}

fn theme_swap_rebuilds_displayed_paragraph_attributes(mtm: MainThreadMarker) {
    let themes = ThemeStore::shared().themes();
    let light = themes.iter().find(|theme| theme.name == "Paper Light").expect("Paper Light").clone();
    let dark = themes.iter().find(|theme| theme.name == "Warm Dark").expect("Warm Dark").clone();
    let aqua = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).unwrap();
    let dark_aqua = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).unwrap();
    let light_sheet = std::rc::Rc::new(StyleSheet::new(light, &aqua, None));
    let dark_sheet = std::rc::Rc::new(StyleSheet::new(dark, &dark_aqua, None));
    let storage = text_storage(MARKER_RICH_TEXT);
    let view = MarkdownTextView::new(rect(0.0, 0.0, 720.0, 420.0), &storage, light_sheet, mtm);
    view.update(parse(MARKER_RICH_TEXT), &wholesale(), true);

    view.set_style_sheet(dark_sheet);

    let hidden = hidden(&view);
    expect!(!hidden.is_empty());
    for substitution in hidden {
        expect!(
            filler_matches_storage(&substitution, &storage),
            "theme swap left a cached light-theme filler at {:?}",
            substitution.source_range
        );
    }
}

fn layout_fillers_preserve_source_length(mtm: MainThreadMarker) {
    let (view, _storage) = decorated_view(mtm);
    let hidden = hidden(&view);
    expect!(!hidden.is_empty());
    for substitution in hidden {
        let Some(replacement) = substitution.replacement else { continue };
        expect!(replacement.length() as isize == substitution.source_range.length);
        expect!(replacement.string().to_string().chars().all(|c| c == '\u{2060}'));
    }
}

fn caret_move_leaves_other_paragraph_fillers_intact(mtm: MainThreadMarker) {
    let (view, storage) = decorated_view(mtm);
    let before = hidden(&view);
    let caret = range_of(MARKER_RICH_TEXT, "### Heading three").location + 5;
    view.set_source_selected_ranges(&[upleft_core::NSRange::new(caret, 0)]);

    let revealed_paragraph = view.paragraph_range_containing(caret);
    let after = hidden(&view);
    for substitution in &after {
        if upleft_core::ns_range::ns_intersection_range(substitution.source_range, revealed_paragraph).length != 0 {
            continue;
        }
        expect!(
            filler_matches_storage(substitution, &storage),
            "filler at {:?} drifted after a caret move",
            substitution.source_range
        );
    }
    expect!(after.len() <= before.len());
}
