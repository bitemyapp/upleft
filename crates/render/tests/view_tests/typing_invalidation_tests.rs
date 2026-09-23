//! Port of `TypingInvalidationTests.swift`: path-existence refreshes stay
//! bounded and consistent across split panes, parse commits keep attribute
//! and overlay invalidation local, and typing keeps rendered objects and the
//! cursor position aligned until the next parse.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSTextStorage, NSTextStorageDelegate, NSTextStorageEditActions};
use objc2_foundation::{NSNumber, NSString};
use upleft_core::{DirtySet, NSRange};
use upleft_render::appkit_compat::attribute_value;
use upleft_render::render_contracts::attribute_keys;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("typing_split_path_caches_invalidate_together", split_path_caches_invalidate_together),
    ("typing_dense_path_refresh_is_chunked", dense_path_refresh_is_chunked),
    ("typing_cancelled_dense_path_clear_restarts", cancelled_dense_path_clear_restarts),
    ("typing_document_typing_keeps_attribute_invalidation_local", document_typing_keeps_attribute_invalidation_local),
    ("typing_keeps_overlay_invalidation_local", typing_keeps_overlay_invalidation_local),
    ("typing_projects_rendered_objects_across_the_edit", typing_projects_rendered_objects_across_the_edit),
    ("typing_cursor_position_tracks_edits", cursor_position_tracks_edits_without_scanning_the_document),
];

struct PathExistenceProbe {
    calls: Cell<usize>,
    exists: Cell<bool>,
}

impl MarkdownTextViewDelegate for PathExistenceProbe {
    fn path_exists_for(&self, _view: &MarkdownTextView, _token: &upleft_core::model::PathToken) -> bool {
        self.calls.set(self.calls.get() + 1);
        self.exists.get()
    }
}

fn probe() -> (Rc<PathExistenceProbe>, Rc<dyn MarkdownTextViewDelegate>) {
    let probe = Rc::new(PathExistenceProbe { calls: Cell::new(0), exists: Cell::new(true) });
    let as_dyn: Rc<dyn MarkdownTextViewDelegate> = probe.clone();
    (probe, as_dyn)
}

fn path_exists(storage: &NSTextStorage, location: isize) -> Option<bool> {
    attribute_value(storage, attribute_keys::dr_path_exists(), location as usize)
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .map(|number| number.boolValue())
}

fn split_path_caches_invalidate_together(mtm: MainThreadMarker) {
    let text = "`fixtures/report.md`";
    let document = parse(text);
    let token = document.path_tokens.first().cloned().expect("a path token");
    let storage = text_storage(text);
    let primary = MarkdownTextView::with_storage(rect(0.0, 0.0, 0.0, 0.0), &storage, mtm);
    let split = MarkdownTextView::with_storage(rect(0.0, 0.0, 0.0, 0.0), &storage, mtm);
    let (probe, delegate) = probe();
    probe.exists.set(false);
    primary.set_markdown_delegate(Some(Rc::downgrade(&delegate)));
    split.set_markdown_delegate(Some(Rc::downgrade(&delegate)));
    primary.update(document.clone(), &wholesale(), true);
    split.update(document.clone(), &wholesale(), true);
    expect!(path_exists(&storage, token.range.location) == Some(false));

    probe.exists.set(true);
    primary.invalidate_path_existence_cache();
    split.invalidate_path_existence_cache();
    primary.refresh_path_existence(None);
    expect!(path_exists(&storage, token.range.location) == Some(true));

    split.update(document, &DirtySet::new(vec![token.range], false), true);
    expect!(path_exists(&storage, token.range.location) == Some(true));
}

fn dense_path_refresh_is_chunked(mtm: MainThreadMarker) {
    let text = (0..512).map(|index| format!("`fixtures/file-{index}.md`")).collect::<Vec<_>>().join("\n");
    let document = parse(&text);
    expect!(document.path_tokens.len() == 512);
    let storage = text_storage(&text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 720.0, 420.0), &storage, mtm);
    view.update(document.clone(), &wholesale(), true);
    let (probe, delegate) = probe();
    view.set_markdown_delegate(Some(Rc::downgrade(&delegate)));

    view.refresh_path_existence(None);
    expect!(probe.calls.get() == 128, "the first main-loop turn must stay bounded: {}", probe.calls.get());
    let total = document.path_tokens.len();
    pump_main_queue(|| probe.calls.get() >= total, Duration::from_secs(2));
    expect!(probe.calls.get() == total);
}

fn cancelled_dense_path_clear_restarts(mtm: MainThreadMarker) {
    let text = (0..512).map(|index| format!("`missing/file-{index}.md`")).collect::<Vec<_>>().join("\n");
    let document = parse(&text);
    let storage = text_storage(&text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 0.0, 0.0), &storage, mtm);
    let (probe, delegate) = probe();
    probe.exists.set(false);
    view.set_markdown_delegate(Some(Rc::downgrade(&delegate)));
    view.update(document.clone(), &wholesale(), true);

    probe.exists.set(true);
    view.refresh_path_existence(None);
    expect!(probe.calls.get() >= 128);
    // A parse commit cancels the remaining scheduled batches.
    view.update(document.clone(), &DirtySet::new(vec![document.path_tokens[0].range], false), true);
    view.refresh_path_existence(None);

    let all_neutral = || document.path_tokens.iter().all(|token| path_exists(&storage, token.range.location) == Some(true));
    expect!(pump_main_queue(all_neutral, Duration::from_secs(2)), "the restarted clear left stale missing-path attributes");
}

// MARK: - Attribute-edit probe

#[derive(Default)]
pub struct EditProbeIvars {
    attribute_edits: RefCell<Vec<NSRange>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate method
    // keeps AppKit's signature. No Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftTextStorageEditProbe"]
    #[ivars = EditProbeIvars]
    struct TextStorageEditProbe;

    unsafe impl NSObjectProtocol for TextStorageEditProbe {}

    unsafe impl NSTextStorageDelegate for TextStorageEditProbe {
        #[unsafe(method(textStorage:didProcessEditing:range:changeInLength:))]
        fn did_process_editing(
            &self,
            _storage: &NSTextStorage,
            edited_mask: NSTextStorageEditActions,
            edited_range: objc2_foundation::NSRange,
            _delta: isize,
        ) {
            if edited_mask.contains(NSTextStorageEditActions::EditedAttributes) {
                self.ivars()
                    .attribute_edits
                    .borrow_mut()
                    .push(NSRange::new(edited_range.location as isize, edited_range.length as isize));
            }
        }
    }
);

impl TextStorageEditProbe {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(EditProbeIvars::default());
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

fn sections(paragraph: &str) -> String {
    (0..80).map(|index| format!("## Section {index}\n\n{paragraph}")).collect::<Vec<_>>().join("\n\n")
}

fn document_typing_keeps_attribute_invalidation_local(mtm: MainThreadMarker) {
    let text = sections("A paragraph whose attributes should remain outside the dirty edit scope.");
    let storage = text_storage(&text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 720.0, 420.0), &storage, mtm);
    let document = parse(&text);
    view.update(document.clone(), &wholesale(), true);

    let probe = TextStorageEditProbe::new(mtm);
    storage.setDelegate(Some(ProtocolObject::from_ref(&*probe)));
    view.update(document, &DirtySet::new(vec![NSRange::new(4, 8)], false), true);
    storage.setDelegate(None);

    let edits = probe.ivars().attribute_edits.borrow().clone();
    let length = storage.length() as isize;
    expect!(!edits.is_empty());
    expect!(edits.iter().all(|edit| edit.length < length / 4), "edits {edits:?}");
}

fn typing_keeps_overlay_invalidation_local(mtm: MainThreadMarker) {
    let text = sections("match in a paragraph whose overlay is outside the edit scope.");
    let storage = text_storage(&text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 720.0, 420.0), &storage, mtm);
    let document = parse(&text);
    view.update(document.clone(), &wholesale(), true);

    let source = NSString::from_str(&text);
    let mut hits: Vec<NSRange> = Vec::new();
    let mut cursor = 0usize;
    while cursor < source.length() {
        let search = objc2_foundation::NSRange::new(cursor, source.length() - cursor);
        let hit = source.rangeOfString_options_range(
            &NSString::from_str("match"),
            objc2_foundation::NSStringCompareOptions(0),
            search,
        );
        if hit.location == objc2_foundation::NSNotFound as usize {
            break;
        }
        hits.push(NSRange::new(hit.location as isize, hit.length as isize));
        cursor = hit.location + hit.length;
    }
    view.set_search_hits(hits);

    let probe = TextStorageEditProbe::new(mtm);
    storage.setDelegate(Some(ProtocolObject::from_ref(&*probe)));
    view.update(document, &DirtySet::new(vec![NSRange::new(4, 8)], false), true);
    storage.setDelegate(None);

    let edits = probe.ivars().attribute_edits.borrow().clone();
    let length = storage.length() as isize;
    expect!(!edits.is_empty());
    expect!(edits.iter().all(|edit| edit.length < length / 4), "edits {edits:?}");
}

fn typing_projects_rendered_objects_across_the_edit(mtm: MainThreadMarker) {
    let text = "Intro.\n\nMath $x^2$ and reference [^1].\n\n[^1]: Note.\n\nTail.";
    let storage = text_storage(text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 720.0, 420.0), &storage, mtm);
    view.update(parse(text), &wholesale(), true);

    let visible = |view: &MarkdownTextView| -> Vec<NSRange> {
        view.current_display_map()
            .substitutions()
            .iter()
            .filter(|substitution| !substitution.is_hidden && !substitution.is_hard_wrap_reflow)
            .map(|substitution| substitution.source_range)
            .collect()
    };
    let before = visible(&view);
    expect!(before.len() >= 2);
    expect!(view.perform_source_edit(NSRange::new(0, 0), "Z", "Edit"));
    let projected = visible(&view);
    expect!(projected.len() == before.len());
    expect!(
        before
            .iter()
            .zip(&projected)
            .all(|(old, new)| new.location == old.location + 1 && new.length == old.length),
        "{before:?} → {projected:?}"
    );
}

fn cursor_position_tracks_edits_without_scanning_the_document(mtm: MainThreadMarker) {
    let text = "first\nsecond\nthird";
    let storage = text_storage(text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 720.0, 420.0), &storage, mtm);
    view.update(parse(text), &wholesale(), true);
    expect!(view.source_position(8) == (2, 3));
    expect!(view.perform_source_edit(NSRange::new(0, 0), "new\n", "Edit"));
    expect!(view.source_position(12) == (3, 3));
}
