//! Port of `DropAndQuickLookTests.swift`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSDragOperation, NSDraggingFormation, NSDraggingInfo, NSEvent, NSEventModifierFlags, NSEventType, NSImage,
    NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeString, NSSpringLoadingHighlight, NSWindow,
};
use objc2_foundation::{NSArray, NSInteger, NSPoint, NSString, NSURL};
use upleft_core::NSRange;
use upleft_render::appkit_compat::RectExt;
use upleft_render::render_contracts::RenderMode;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::{ContextTarget, ContextTargetKind, DocumentDrop, MarkdownTextViewDelegate};

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("drop_reports_source_coordinates", drop_reports_source_coordinates),
    ("drop_hover_and_drop_use_the_same_coordinate_space", hover_and_drop_use_the_same_coordinate_space),
    ("drop_resolves_the_release_point", drop_resolves_the_release_point),
    ("drop_empty_document_drops_at_zero", empty_document_drops_at_zero),
    ("drop_editable_surface_registers_drop_types", editable_surface_registers_drop_types),
    ("drop_read_only_mode_unregisters_drop_types", read_only_mode_unregisters_drop_types),
    ("drop_read_only_surface_claims_nothing", read_only_surface_claims_nothing),
    ("drop_text_drags_are_not_claimed", text_drags_are_not_claimed),
    ("drop_indicator_is_always_cleared", indicator_is_always_cleared),
    ("quicklook_force_click_resolves_previewable_targets", force_click_resolves_previewable_targets),
    ("quicklook_force_click_on_a_word_falls_through", force_click_on_a_word_falls_through),
    ("quicklook_non_file_targets_are_refused", non_file_targets_are_refused),
    ("quicklook_space_never_previews_while_editing", space_never_previews_while_editing),
    ("quicklook_space_previews_on_a_read_only_surface", space_previews_on_a_read_only_surface),
    ("quicklook_space_without_a_selection_falls_through", space_without_a_selection_falls_through),
    ("quicklook_modified_space_is_not_a_preview", modified_space_is_not_a_preview),
    ("quicklook_selection_targets_follow_the_caret", selection_targets_follow_the_caret),
];

// MARK: - Harness

struct Delegate {
    accepts: Cell<bool>,
    asked_to_accept: RefCell<Vec<isize>>,
    drops: RefCell<Vec<isize>>,
    quick_looks: RefCell<Vec<ContextTarget>>,
    handles_quick_look: Cell<bool>,
}

impl MarkdownTextViewDelegate for Delegate {
    fn can_accept_drop(&self, _view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        self.asked_to_accept.borrow_mut().push(drop.source_offset);
        self.accepts.get()
    }
    fn did_accept_drop(&self, _view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        self.drops.borrow_mut().push(drop.source_offset);
        self.accepts.get()
    }
    fn wants_quick_look_for(&self, _view: &MarkdownTextView, target: &ContextTarget) -> bool {
        self.quick_looks.borrow_mut().push(target.clone());
        self.handles_quick_look.get()
    }
}

pub struct SyntheticDragIvars {
    pasteboard: Retained<NSPasteboard>,
    location: Cell<NSPoint>,
    valid_items: Cell<NSInteger>,
    formation: Cell<NSDraggingFormation>,
    animates: Cell<bool>,
}

define_class!(
    /// A minimal `NSDraggingInfo` (`SyntheticDrag`).
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SyntheticDrag"]
    #[ivars = SyntheticDragIvars]
    struct SyntheticDrag;

    unsafe impl NSObjectProtocol for SyntheticDrag {}

    unsafe impl NSDraggingInfo for SyntheticDrag {
        #[unsafe(method_id(draggingDestinationWindow))]
        fn dragging_destination_window(&self) -> Option<Retained<NSWindow>> {
            None
        }
        #[unsafe(method(draggingSourceOperationMask))]
        fn dragging_source_operation_mask(&self) -> NSDragOperation {
            NSDragOperation::Copy | NSDragOperation::Generic
        }
        #[unsafe(method(draggingLocation))]
        fn dragging_location(&self) -> NSPoint {
            self.ivars().location.get()
        }
        #[unsafe(method(draggedImageLocation))]
        fn dragged_image_location(&self) -> NSPoint {
            self.ivars().location.get()
        }
        #[unsafe(method_id(draggedImage))]
        fn dragged_image(&self) -> Option<Retained<NSImage>> {
            None
        }
        #[unsafe(method_id(draggingPasteboard))]
        fn dragging_pasteboard(&self) -> Retained<NSPasteboard> {
            self.ivars().pasteboard.clone()
        }
        #[unsafe(method_id(draggingSource))]
        fn dragging_source(&self) -> Option<Retained<AnyObject>> {
            None
        }
        #[unsafe(method(draggingSequenceNumber))]
        fn dragging_sequence_number(&self) -> NSInteger {
            1
        }
        #[unsafe(method(slideDraggedImageTo:))]
        fn slide_dragged_image_to(&self, _point: NSPoint) {}
        #[unsafe(method_id(namesOfPromisedFilesDroppedAtDestination:))]
        fn names_of_promised_files(&self, _destination: &NSURL) -> Option<Retained<NSArray<NSString>>> {
            None
        }
        #[unsafe(method(draggingFormation))]
        fn dragging_formation(&self) -> NSDraggingFormation {
            self.ivars().formation.get()
        }
        #[unsafe(method(setDraggingFormation:))]
        fn set_dragging_formation(&self, formation: NSDraggingFormation) {
            self.ivars().formation.set(formation);
        }
        #[unsafe(method(animatesToDestination))]
        fn animates_to_destination(&self) -> bool {
            self.ivars().animates.get()
        }
        #[unsafe(method(setAnimatesToDestination:))]
        fn set_animates_to_destination(&self, value: bool) {
            self.ivars().animates.set(value);
        }
        #[unsafe(method(numberOfValidItemsForDrop))]
        fn number_of_valid_items_for_drop(&self) -> NSInteger {
            self.ivars().valid_items.get()
        }
        #[unsafe(method(setNumberOfValidItemsForDrop:))]
        fn set_number_of_valid_items_for_drop(&self, value: NSInteger) {
            self.ivars().valid_items.set(value);
        }
        #[unsafe(method(enumerateDraggingItemsWithOptions:forView:classes:searchOptions:usingBlock:))]
        fn enumerate_dragging_items(
            &self,
            _options: usize,
            _view: Option<&AnyObject>,
            _classes: &AnyObject,
            _search_options: &AnyObject,
            _block: &AnyObject,
        ) {
        }
        #[unsafe(method(springLoadingHighlight))]
        fn spring_loading_highlight(&self) -> NSSpringLoadingHighlight {
            NSSpringLoadingHighlight::None
        }
        #[unsafe(method(resetSpringLoading))]
        fn reset_spring_loading(&self) {}
    }
);

impl SyntheticDrag {
    fn new(pasteboard: Retained<NSPasteboard>, location: NSPoint, mtm: MainThreadMarker) -> Retained<SyntheticDrag> {
        let this = Self::alloc(mtm).set_ivars(SyntheticDragIvars {
            pasteboard,
            location: Cell::new(location),
            valid_items: Cell::new(1),
            formation: Cell::new(NSDraggingFormation::Default),
            animates: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn info(&self) -> &ProtocolObject<dyn NSDraggingInfo> {
        ProtocolObject::from_ref(self)
    }
}

fn document() -> String {
    let mut lines: Vec<String> = vec!["# A Title With Markers".into(), String::new()];
    for index in 0..6 {
        lines.push(format!("## Section {index}"));
        lines.push(String::new());
        lines.push(format!(
            "Paragraph {index} carries **strong emphasis** and _light emphasis_ plus `inline code` and a [link](https://example.com/some/long/path) before the rest of the sentence runs on to the end of the line."
        ));
        lines.push(String::new());
    }
    lines.push("The very last paragraph[^1][^2][^3], which has a TARGETWORD in it.".into());
    lines.push(String::new());
    for identifier in ["1", "2", "3"] {
        lines.push(format!("[^{identifier}]: A footnote definition."));
    }
    lines.push(String::new());
    lines.join("\n")
}

const LINKED_DOCUMENT: &str = "# Notes\n\nSee [the design](design.md) and `src/auth/session.ts` for the details.\n\n![A diagram](diagram.png)\n\nAn ordinary sentence with an unremarkable word in it.";

struct Harness {
    container: Retained<MarkdownContainerView>,
    view: Retained<MarkdownTextView>,
    delegate: Rc<Delegate>,
    _as_delegate: Rc<dyn MarkdownTextViewDelegate>,
    text: String,
}

fn harness(mode: RenderMode, text: &str, mtm: MainThreadMarker) -> Harness {
    let storage = text_storage(text);
    let container = MarkdownContainerView::new(&storage, fallback_sheet(), mtm);
    container.setFrame(rect(0.0, 0.0, 900.0, 600.0));
    container.layoutSubtreeIfNeeded();
    let view = container.text_view().clone();
    view.set_mode(mode);
    let delegate = Rc::new(Delegate {
        accepts: Cell::new(true),
        asked_to_accept: RefCell::new(Vec::new()),
        drops: RefCell::new(Vec::new()),
        quick_looks: RefCell::new(Vec::new()),
        handles_quick_look: Cell::new(true),
    });
    let as_delegate: Rc<dyn MarkdownTextViewDelegate> = delegate.clone();
    view.set_markdown_delegate(Some(Rc::downgrade(&as_delegate)));
    view.update(parse(text), &wholesale(), true);
    container.layoutSubtreeIfNeeded();
    Harness { container, view, delegate, _as_delegate: as_delegate, text: text.to_owned() }
}

fn pasteboard_name() -> Retained<NSString> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    NSString::from_str(&format!("DropTests-{}-{nanos}", std::process::id()))
}

fn file_pasteboard(path: &str) -> Retained<NSPasteboard> {
    let pasteboard = NSPasteboard::pasteboardWithName(&pasteboard_name());
    pasteboard.clearContents();
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let objects = NSArray::from_retained_slice(&[ProtocolObject::from_retained(url)]);
    pasteboard.writeObjects(&objects);
    pasteboard
}

fn text_pasteboard(string: &str) -> Retained<NSPasteboard> {
    let pasteboard = NSPasteboard::pasteboardWithName(&pasteboard_name());
    pasteboard.clearContents();
    pasteboard.setString_forType(&NSString::from_str(string), unsafe { NSPasteboardTypeString });
    pasteboard
}

/// A point inside the glyphs of `offset`, in window coordinates.
fn drag_location(offset: isize, view: &MarkdownTextView) -> NSPoint {
    let rect = view.rect_for_offset(offset).unwrap_or_else(|| panic!("no geometry for offset {offset}"));
    view.convertPoint_toView(NSPoint::new(rect.min_x() + 1.0, rect.mid_y()), None)
}

fn entered(view: &MarkdownTextView, drag: &SyntheticDrag) -> NSDragOperation {
    unsafe { msg_send![view, draggingEntered: drag.info()] }
}

fn updated(view: &MarkdownTextView, drag: &SyntheticDrag) -> NSDragOperation {
    unsafe { msg_send![view, draggingUpdated: drag.info()] }
}

fn perform(view: &MarkdownTextView, drag: &SyntheticDrag) -> bool {
    unsafe { msg_send![view, performDragOperation: drag.info()] }
}

fn exited(view: &MarkdownTextView, drag: &SyntheticDrag) {
    let _: () = unsafe { msg_send![view, draggingExited: Some(drag.info())] };
}

fn substring(text: &str, range: NSRange) -> String {
    NSString::from_str(text)
        .substringWithRange(objc2_foundation::NSRange::new(range.location as usize, range.length as usize))
        .to_string()
}

fn registered(view: &MarkdownTextView) -> Vec<String> {
    view.registeredDraggedTypes().iter().map(|kind| kind.to_string()).collect()
}

// MARK: - The conversion this whole feature rests on

fn drop_reports_source_coordinates(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    let target = range_of(&h.text, "TARGETWORD").location;
    let location = drag_location(target, &h.view);
    let view_point = h.view.convertPoint_fromView(location, None);
    let display_offset: usize = unsafe { msg_send![&*h.view, characterIndexForInsertionAtPoint: view_point] };
    let source_offset = h.view.source_offset_at(view_point);
    expect!(
        display_offset as isize != source_offset,
        "the fixture no longer shifts the two spaces apart, so this test proves nothing"
    );
    let drag = SyntheticDrag::new(file_pasteboard("/tmp/a.png"), location, mtm);
    expect!(entered(&h.view, &drag) == NSDragOperation::Copy);
    expect!(perform(&h.view, &drag));
    let reported = *h.delegate.drops.borrow().last().expect("a drop");
    expect!(reported == source_offset);
    expect!(reported != display_offset as isize, "the drop reported a TextKit offset as a source offset");
    expect!(substring(&h.text, NSRange::new(reported, 10)) == "TARGETWORD");
}

fn hover_and_drop_use_the_same_coordinate_space(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    let target = range_of(&h.text, "TARGETWORD").location;
    let drag = SyntheticDrag::new(file_pasteboard("/tmp/a.png"), drag_location(target, &h.view), mtm);
    let _ = entered(&h.view, &drag);
    let hovered = h.view.drop_insertion_offset().expect("a hover offset");
    expect!(updated(&h.view, &drag) == NSDragOperation::Copy);
    expect!(h.view.drop_insertion_offset() == Some(hovered));
    expect!(perform(&h.view, &drag));
    expect!(h.delegate.drops.borrow().last() == Some(&hovered));
    expect!(*h.delegate.asked_to_accept.borrow() == vec![hovered], "the claim must be asked exactly once per drag");
}

fn drop_resolves_the_release_point(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    let first = range_of(&h.text, "Paragraph 2 carries").location;
    let second = range_of(&h.text, "TARGETWORD").location;
    let hover = SyntheticDrag::new(file_pasteboard("/tmp/a.png"), drag_location(first, &h.view), mtm);
    let _ = entered(&h.view, &hover);
    let hovered = h.view.drop_insertion_offset().expect("a hover offset");
    hover.ivars().location.set(drag_location(second, &h.view));
    expect!(perform(&h.view, &hover));
    let dropped = *h.delegate.drops.borrow().last().expect("a drop");
    expect!(dropped != hovered);
    expect!(substring(&h.text, NSRange::new(dropped, 10)) == "TARGETWORD");
}

fn empty_document_drops_at_zero(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, "", mtm);
    let bounds = h.container.bounds();
    let location = h.view.convertPoint_toView(NSPoint::new(bounds.mid_x(), 40.0), None);
    let drag = SyntheticDrag::new(file_pasteboard("/tmp/a.png"), location, mtm);
    expect!(entered(&h.view, &drag) == NSDragOperation::Copy);
    expect!(perform(&h.view, &drag));
    expect!(*h.delegate.drops.borrow() == vec![0]);
}

// MARK: - Which drags the surface claims

fn editable_surface_registers_drop_types(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    let types = registered(&h.view);
    for kind in MarkdownTextView::document_drop_types() {
        expect!(types.contains(&kind.to_string()), "{kind} is not registered");
    }
    let before = registered(&h.view);
    let _: () = unsafe { msg_send![&*h.view, updateDragTypeRegistration] };
    expect!(registered(&h.view) == before);
}

fn file_url_type() -> String {
    unsafe { NSPasteboardTypeFileURL }.to_string()
}

fn read_only_mode_unregisters_drop_types(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    expect!(registered(&h.view).contains(&file_url_type()));
    h.view.set_mode(RenderMode::Read);
    expect!(!registered(&h.view).contains(&file_url_type()));
    h.view.set_mode(RenderMode::Live);
    expect!(registered(&h.view).contains(&file_url_type()));
}

fn read_only_surface_claims_nothing(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Read, &document(), mtm);
    expect!(!h.view.isEditable());
    expect!(!registered(&h.view).contains(&file_url_type()));
    let location = drag_location(range_of(&h.text, "TARGETWORD").location, &h.view);
    let drag = SyntheticDrag::new(file_pasteboard("/tmp/a.png"), location, mtm);
    let _ = entered(&h.view, &drag);
    expect!(h.delegate.asked_to_accept.borrow().is_empty());
    expect!(h.view.drop_insertion_offset().is_none());
}

fn text_drags_are_not_claimed(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    let location = drag_location(range_of(&h.text, "TARGETWORD").location, &h.view);
    let drag = SyntheticDrag::new(text_pasteboard("some words"), location, mtm);
    h.delegate.accepts.set(false);
    let _ = entered(&h.view, &drag);
    expect!(!h.view.claims_active_drag());
    expect!(h.view.drop_insertion_offset().is_none());
    expect!(h.delegate.drops.borrow().is_empty());
}

fn indicator_is_always_cleared(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, &document(), mtm);
    let location = drag_location(range_of(&h.text, "TARGETWORD").location, &h.view);
    let pasteboard = file_pasteboard("/tmp/a.png");
    let leaving = SyntheticDrag::new(pasteboard.clone(), location, mtm);
    let _ = entered(&h.view, &leaving);
    expect!(h.view.drop_insertion_offset().is_some());
    exited(&h.view, &leaving);
    expect!(h.view.drop_insertion_offset().is_none());
    expect!(!h.view.claims_active_drag());
    let landing = SyntheticDrag::new(pasteboard, location, mtm);
    let _ = entered(&h.view, &landing);
    expect!(perform(&h.view, &landing));
    expect!(h.view.drop_insertion_offset().is_none());
    expect!(!h.view.claims_active_drag());
}

// MARK: - Quick Look targets

fn view_point(view: &MarkdownTextView, offset: isize) -> NSPoint {
    view.convertPoint_fromView(drag_location(offset, view), None)
}

fn force_click_resolves_previewable_targets(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, LINKED_DOCUMENT, mtm);
    let link_point = view_point(&h.view, range_of(&h.text, "the design").location);
    let Some(ContextTargetKind::Link(destination)) = h.view.quick_look_target(link_point).map(|target| target.kind) else {
        panic!("a force click on link text must resolve to a link");
    };
    expect!(destination == "design.md");
    let path_point = view_point(&h.view, range_of(&h.text, "src/auth/session.ts").location + 4);
    let Some(ContextTargetKind::PathToken(token)) = h.view.quick_look_target(path_point).map(|target| target.kind) else {
        panic!("a force click on a path token must resolve to a path token");
    };
    expect!(token.raw_path == "src/auth/session.ts");
}

fn force_click_on_a_word_falls_through(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, LINKED_DOCUMENT, mtm);
    let point = view_point(&h.view, range_of(&h.text, "unremarkable").location + 3);
    expect!(h.view.quick_look_target(point).is_none());
}

fn non_file_targets_are_refused(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, LINKED_DOCUMENT, mtm);
    let heading = view_point(&h.view, range_of(&h.text, "Notes").location);
    expect!(h.view.quick_look_target(heading).is_none());
}

fn space_event(modifiers: NSEventModifierFlags) -> Retained<NSEvent> {
    let space = NSString::from_str(" ");
    NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        modifiers,
        0.0,
        0,
        None,
        &space,
        &space,
        false,
        49,
    )
    .expect("a key event")
}

fn space_never_previews_while_editing(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, LINKED_DOCUMENT, mtm);
    expect!(h.view.isEditable());
    h.view.set_source_selected_ranges(&[range_of(&h.text, "the design")]);
    expect!(h.view.quick_look_target_at_selection().is_some(), "the selection is on a link");
    expect!(!h.view.handle_quick_look_space(&space_event(NSEventModifierFlags::empty())));
    expect!(h.delegate.quick_looks.borrow().is_empty());
}

fn space_previews_on_a_read_only_surface(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Read, LINKED_DOCUMENT, mtm);
    h.view.set_source_selected_ranges(&[range_of(&h.text, "the design")]);
    expect!(h.view.handle_quick_look_space(&space_event(NSEventModifierFlags::empty())));
    let last = h.delegate.quick_looks.borrow().last().cloned();
    let Some(ContextTarget { kind: ContextTargetKind::Link(destination), .. }) = last else {
        panic!("Space on a selected link must report a link target");
    };
    expect!(destination == "design.md");
}

fn space_without_a_selection_falls_through(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Read, LINKED_DOCUMENT, mtm);
    expect!(h.view.source_selected_range().length == 0);
    expect!(!h.view.handle_quick_look_space(&space_event(NSEventModifierFlags::empty())));
    expect!(h.delegate.quick_looks.borrow().is_empty());
}

fn modified_space_is_not_a_preview(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Read, LINKED_DOCUMENT, mtm);
    h.view.set_source_selected_ranges(&[range_of(&h.text, "the design")]);
    expect!(!h.view.handle_quick_look_space(&space_event(NSEventModifierFlags::Option)));
    expect!(h.delegate.quick_looks.borrow().is_empty());
}

fn selection_targets_follow_the_caret(mtm: MainThreadMarker) {
    let h = harness(RenderMode::Live, LINKED_DOCUMENT, mtm);
    h.view.set_source_selected_ranges(&[NSRange::new(range_of(&h.text, "the design").location + 2, 0)]);
    expect!(h.view.quick_look_target_at_selection().is_some());
    h.view.set_source_selected_ranges(&[NSRange::new(range_of(&h.text, "unremarkable").location + 3, 0)]);
    expect!(h.view.quick_look_target_at_selection().is_none());
}

