//! Port of `SmartPasteIntegrationTests.swift`.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeRTF, NSPasteboardTypeString,
    NSPasteboardTypeURL, NSResponder, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSData, NSString, NSUndoManager};
use upleft_core::{NSRange, ZoomLevel};
use upleft_render::appkit_compat::attribute_value;
use upleft_render::render_contracts::{RenderMode, SourceFocus, attribute_keys};
use upleft_render::view::markdown_smart_paste::{
    MarkdownPasteContext, MarkdownPasteMode, MarkdownPastePayload, MarkdownSmartPaste, apple_web_archive_type,
    downright_markdown_type,
};
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("paste_pasteboard_priority", pasteboard_priority),
    ("paste_private_markdown_priority", private_markdown_priority),
    ("paste_visible_copy_with_markdown_alternate", visible_copy_with_markdown_alternate),
    ("paste_source_focus_lifecycle", source_focus_lifecycle),
    ("paste_html_only_clipboard", html_only_clipboard),
    ("paste_safari_html_round_trip", safari_html_round_trip),
    ("paste_source_edit_and_undo", source_edit_and_undo),
    ("paste_repeated_document_editing", repeated_document_editing),
    ("paste_app_kit_editing_entry_points", app_kit_editing_entry_points),
    ("paste_transient_edit_projection", transient_edit_projection),
    ("paste_transient_heading_edit_keeps_marker_hidden", transient_heading_edit_keeps_marker_hidden),
    ("paste_transient_edit_projection_keeps_hard_wrap_reflow", transient_edit_projection_keeps_hard_wrap_reflow),
    (
        "paste_transient_edit_projection_keeps_touched_hard_wrap_block",
        transient_edit_projection_keeps_touched_hard_wrap_block,
    ),
    (
        "paste_transient_structural_edit_drops_touched_hard_wrap_block",
        transient_structural_edit_drops_touched_hard_wrap_block,
    ),
    ("paste_literal_contexts_bypass_transforms", literal_contexts_bypass_transforms),
    ("paste_inline_literal_boundaries", inline_literal_boundaries),
    ("paste_source_mode_passthrough", source_mode_passthrough),
];

pub struct UndoHostIvars {
    manager: Retained<NSUndoManager>,
}

define_class!(
    /// `UndoManagerHostView`: a view that owns an undo manager.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UndoManagerHostView"]
    #[ivars = UndoHostIvars]
    struct UndoManagerHostView;

    unsafe impl NSObjectProtocol for UndoManagerHostView {}

    impl UndoManagerHostView {
        #[unsafe(method_id(undoManager))]
        fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
            Some(self.ivars().manager.clone())
        }
    }
);

fn isolated_pasteboard() -> Retained<NSPasteboard> {
    let name = NSString::from_str(&format!("UpleftTests.{}.{}", std::process::id(), rand_suffix()));
    NSPasteboard::pasteboardWithName(&name)
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

fn set_string(pasteboard: &NSPasteboard, value: &str, kind: &NSString) {
    pasteboard.setString_forType(&NSString::from_str(value), kind);
}

fn view_for(source: &str, mode: RenderMode, mtm: MainThreadMarker) -> Retained<MarkdownTextView> {
    let (view, _storage) = view_with(source, rect(0.0, 0.0, 640.0, 400.0), mtm);
    view.set_mode(mode);
    view.update(parse(source), &wholesale(), true);
    view
}

fn text_of(view: &MarkdownTextView) -> String {
    storage_string(&unsafe { view.textStorage() }.expect("storage"))
}

fn pasteboard_priority(_mtm: MainThreadMarker) {
    let pasteboard = isolated_pasteboard();
    pasteboard.clearContents();
    unsafe {
        set_string(&pasteboard, "www.example.com", NSPasteboardTypeURL);
        set_string(&pasteboard, "<p>Browser title</p>", NSPasteboardTypeHTML);
        set_string(&pasteboard, "fallback", NSPasteboardTypeString);
    }
    let payload = MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Smart);
    expect!(payload == Some(MarkdownPastePayload::Text("fallback".into())), "{payload:?}");
    expect!(
        MarkdownSmartPaste::replacement(&payload.unwrap(), "title", MarkdownPasteContext::Markdown, MarkdownPasteMode::Smart)
            == "fallback"
    );
    let markdown_payload = MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Markdown);
    expect!(
        markdown_payload == Some(MarkdownPastePayload::Html("<p>Browser title</p>".into(), "fallback".into())),
        "{markdown_payload:?}"
    );
    let replacement = MarkdownSmartPaste::replacement(
        &markdown_payload.unwrap(),
        "",
        MarkdownPasteContext::Markdown,
        MarkdownPasteMode::Markdown,
    );
    expect!(replacement == "Browser title", "{replacement:?}");
    expect!(
        MarkdownSmartPaste::replacement(
            &MarkdownPastePayload::Url("javascript://alert(1)".into()),
            "title",
            MarkdownPasteContext::Markdown,
            MarkdownPasteMode::Smart
        ) == "javascript://alert(1)"
    );
    expect!(
        MarkdownSmartPaste::replacement(
            &MarkdownPastePayload::Url("https://example.com/a(b)".into()),
            "title",
            MarkdownPasteContext::Markdown,
            MarkdownPasteMode::Smart
        ) == "[title](<https://example.com/a(b)>)"
    );
}

fn private_markdown_priority(_mtm: MainThreadMarker) {
    let pasteboard = isolated_pasteboard();
    pasteboard.clearContents();
    set_string(&pasteboard, "**lossless**", &downright_markdown_type());
    unsafe {
        set_string(&pasteboard, "https://example.com", NSPasteboardTypeURL);
        set_string(&pasteboard, "lossless", NSPasteboardTypeString);
    }
    expect!(
        MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Smart)
            == Some(MarkdownPastePayload::Markdown("**lossless**".into()))
    );
}

fn visible_copy_with_markdown_alternate(mtm: MainThreadMarker) {
    let source = "Before **bold** after";
    let view = view_for(source, RenderMode::Live, mtm);
    view.set_source_selected_ranges(&[range_of(source, "**bold**")]);
    let pasteboard = isolated_pasteboard();
    let types = objc2_foundation::NSArray::<NSString>::new();
    let wrote: bool = unsafe { msg_send![&*view, writeSelectionToPasteboard: &*pasteboard, types: &*types] };
    expect!(wrote);
    let string = pasteboard.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string());
    expect!(string.as_deref() == Some("bold"), "{string:?}");
    let markdown = pasteboard.stringForType(&downright_markdown_type()).map(|s| s.to_string());
    expect!(markdown.as_deref() == Some("**bold**"), "{markdown:?}");
    expect!(pasteboard.dataForType(unsafe { NSPasteboardTypeRTF }).is_some());
}

fn source_focus_lifecycle(mtm: MainThreadMarker) {
    let source = "First **line**.\nSecond line.\n";
    let view = view_for(source, RenderMode::Live, mtm);
    let selection = range_of(source, "line");
    view.focus_source(selection);
    let ns = NSString::from_str(source);
    let expected = upleft_render::appkit_compat::from_ns(
        ns.paragraphRangeForRange(objc2_foundation::NSRange::new(selection.location as usize, selection.length as usize)),
    );
    expect!(view.source_focus() == SourceFocus::Scoped(expected), "{:?}", view.source_focus());
    expect!(view.source_selected_range() == selection);
    let storage = unsafe { view.textStorage() }.unwrap();
    expect!(attribute_value(&storage, attribute_keys::dr_source_focus(), selection.location as usize).is_some());
    expect!(attribute_value(&storage, attribute_keys::dr_hidden(), selection.location as usize).is_none());
    let colour = attribute_value(&storage, upleft_render::appkit_compat::keys::foreground_color(), selection.location as usize)
        .and_then(|value| value.downcast::<objc2_app_kit::NSColor>().ok());
    expect!(colour.is_some_and(|colour| colour.alphaComponent() > 0.0));

    view.clear_source_focus();
    expect!(view.source_focus() == SourceFocus::None);
    expect!(view.mode() == RenderMode::Live);
    view.focus_entire_source();
    expect!(view.source_focus() == SourceFocus::Document);
    expect!(view.mode() == RenderMode::Source);
    view.clear_source_focus();
    expect!(view.source_focus() == SourceFocus::None);
    expect!(view.mode() == RenderMode::Live);
}

fn html_only_clipboard(_mtm: MainThreadMarker) {
    let pasteboard = isolated_pasteboard();
    pasteboard.clearContents();
    unsafe { set_string(&pasteboard, "<p><strong>Only HTML</strong></p>", NSPasteboardTypeHTML) };
    let payload = MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Smart);
    expect!(payload == Some(MarkdownPastePayload::Html("<p><strong>Only HTML</strong></p>".into(), String::new())));
    expect!(
        MarkdownSmartPaste::replacement(&payload.unwrap(), "", MarkdownPasteContext::Markdown, MarkdownPasteMode::Smart)
            == "**Only HTML**"
    );
}

fn safari_html_round_trip(_mtm: MainThreadMarker) {
    let pasteboard = isolated_pasteboard();
    pasteboard.clearContents();
    let html = "<!DOCTYPE html><html><head><style>body { color: red }</style></head><body><!--StartFragment-->\n<div><h2>Heading</h2><p>Intro <strong>bold</strong> and <a href=\"https://example.com\">link</a>.</p>\n<ul><li>first<ul><li>nested <code>code</code></li></ul></li><li>second</li></ul>\n<table><tbody><tr><th>Name</th><th>Count</th></tr><tr><td>Ada</td><td>1</td></tr></tbody></table>\n<script>window.evil = true</script></div><!--EndFragment--></body></html>";
    let flattened = "Heading Intro bold and link. first nested code second Name Count Ada 1";
    unsafe {
        set_string(&pasteboard, html, NSPasteboardTypeHTML);
        set_string(&pasteboard, flattened, NSPasteboardTypeString);
        pasteboard.setData_forType(Some(&NSData::with_bytes(&[0x00, 0x01])), NSPasteboardTypeRTF);
    }
    pasteboard.setData_forType(Some(&NSData::with_bytes(&[0x02, 0x03])), &apple_web_archive_type());

    expect!(MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Smart) == Some(MarkdownPastePayload::Text(flattened.into())));
    let payload = MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Markdown).expect("a payload");
    let replacement =
        MarkdownSmartPaste::replacement(&payload, "", MarkdownPasteContext::Markdown, MarkdownPasteMode::Smart);
    for needle in ["## Heading", "**bold**", "[link](https://example.com)", "- first\n  - nested `code`\n- second", "| Name", "| Ada"] {
        expect!(replacement.contains(needle), "missing {needle:?} in {replacement:?}");
    }
    expect!(!replacement.contains("window.evil"));
    expect!(!replacement.contains("Heading Intro bold and link. first nested code"));
}

fn source_edit_and_undo(mtm: MainThreadMarker) {
    let source = "Select this";
    let view = view_for(source, RenderMode::Live, mtm);
    let host = UndoManagerHostView::alloc(mtm).set_ivars(UndoHostIvars { manager: NSUndoManager::new(mtm) });
    let host: Retained<UndoManagerHostView> = unsafe { msg_send![super(host), initWithFrame: rect(0.0, 0.0, 640.0, 400.0)] };
    host.addSubview(&view);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            host.frame(),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            true,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(&host));
    expect!(window.makeFirstResponder(Some(&view)));
    view.set_source_selected_ranges(&[NSRange::new(0, utf16_len(source))]);

    let pasteboard = isolated_pasteboard();
    pasteboard.clearContents();
    unsafe { set_string(&pasteboard, "<p><em>replacement</em></p>", NSPasteboardTypeHTML) };
    let payload = MarkdownSmartPaste::payload(&pasteboard, MarkdownPasteMode::Smart).unwrap();
    let replacement = MarkdownSmartPaste::replacement(&payload, source, MarkdownPasteContext::Markdown, MarkdownPasteMode::Smart);
    expect!(view.perform_source_edit(view.source_selected_range(), &replacement, "Edit"));
    expect!(text_of(&view) == "*replacement*", "{:?}", text_of(&view));
    expect!(view.source_selected_range() == NSRange::new(13, 0));

    let undo_manager = view.undoManager().expect("undo manager");
    expect!(undo_manager.canUndo());
    undo_manager.undo();
    expect!(text_of(&view) == source, "{:?}", text_of(&view));
    expect!(!undo_manager.canUndo());
    window.orderOut(None);
}

fn repeated_document_editing(mtm: MainThreadMarker) {
    let view = view_for("# Title\n\nBody with **bold** text.\n", RenderMode::Live, mtm);
    expect!(view.isEditable());
    let body_end = range_of(&text_of(&view), "Body").upper_bound();
    view.set_source_selected_ranges(&[NSRange::new(body_end, 0)]);
    for character in " grows".chars() {
        expect!(view.perform_source_edit(view.source_selected_range(), &character.to_string(), "Edit"));
    }
    expect!(text_of(&view) == "# Title\n\nBody grows with **bold** text.\n");
    expect!(view.source_selected_range() == NSRange::new(body_end + 6, 0));
    let _: () = unsafe { msg_send![&*view, deleteBackward: None::<&AnyObject>] };
    expect!(text_of(&view) == "# Title\n\nBody grow with **bold** text.\n");
    expect!(view.source_selected_range() == NSRange::new(body_end + 5, 0));
}

fn app_kit_editing_entry_points(mtm: MainThreadMarker) {
    let source = "# Title\n\nBody with **bold** text.\n";
    let view = view_for(source, RenderMode::Live, mtm);
    let bold = range_of(source, "bold");
    let displayed = view.current_display_map().text_kit_range_for_source(bold);
    let _: () = unsafe {
        msg_send![&*view, setSelectedRange: objc2_foundation::NSRange::new(displayed.location as usize, displayed.length as usize)]
    };
    expect!(view.source_selected_range() == bold, "{:?}", view.source_selected_range());
    let _: () = unsafe { msg_send![&*view, deleteBackward: None::<&AnyObject>] };
    expect!(text_of(&view) == "# Title\n\nBody with **** text.\n", "{:?}", text_of(&view));

    let insertion = range_of(&text_of(&view), "Body").upper_bound();
    view.set_source_selected_ranges(&[NSRange::new(insertion, 0)]);
    let grows = NSString::from_str(" grows");
    let not_found = objc2_foundation::NSRange::new(objc2_foundation::NSNotFound as usize, 0);
    let _: () = unsafe { msg_send![&*view, insertText: &*grows, replacementRange: not_found] };
    expect!(text_of(&view) == "# Title\n\nBody grows with **** text.\n", "{:?}", text_of(&view));
    expect!(view.source_selected_range() == NSRange::new(insertion + 6, 0));
}

fn transient_edit_projection(mtm: MainThreadMarker) {
    let source = "# Title\n\nBody with **bold** text.\n\nTail with _emphasis_.\n";
    let view = view_for(source, RenderMode::Live, mtm);
    view.set_zoom_level(ZoomLevel::Skeleton);
    let body = range_of(source, "Body with **bold** text.\n");
    let tail_marker = range_of(source, "_emphasis_").location;
    expect!(!view.current_display_map().hidden_ranges().is_empty());
    expect!(view.perform_source_edit(NSRange::new(body.location + 4, 0), " edited", "Edit"));
    expect!(view.zoom_level() == ZoomLevel::Everything);
    let projected = view.current_display_map().hidden_ranges();
    let edited_paragraph = view.paragraph_index().paragraph_range_containing(body.location);
    expect!(projected.iter().any(|range| upleft_core::ns_range::ns_intersection_range(*range, edited_paragraph).length > 0));
    expect!(projected.iter().any(|range| range.location >= tail_marker + 7));
    expect!(view.current_display_map().paragraphs.length == unsafe { view.textStorage() }.unwrap().length() as isize);
}

fn transient_heading_edit_keeps_marker_hidden(mtm: MainThreadMarker) {
    let source = "# Title\n\nBody\n";
    let view = view_for(source, RenderMode::Live, mtm);
    let marker = NSRange::new(0, 2);
    expect!(view.current_display_map().hidden_ranges().contains(&marker));
    expect!(view.perform_source_edit(NSRange::new(range_of(source, "Title").upper_bound(), 0), " grows", "Edit"));
    expect!(view.current_display_map().hidden_ranges().contains(&marker));
    expect!(attribute_value(&unsafe { view.textStorage() }.unwrap(), attribute_keys::dr_hidden(), 0).is_some());
}

fn hard_wrap_breaks(view: &MarkdownTextView) -> Vec<isize> {
    view.current_display_map()
        .substitutions()
        .iter()
        .filter(|sub| sub.is_hard_wrap_reflow)
        .map(|sub| sub.source_range.location)
        .collect()
}

fn transient_edit_projection_keeps_hard_wrap_reflow(mtm: MainThreadMarker) {
    let source = "# Title\n\nFirst prose line wraps in the source\nbut remains one rendered paragraph.\n\nSecond prose line also wraps in the source\nand must not flash back to physical lines.";
    let view = view_for(source, RenderMode::Live, mtm);
    let old_break = range_of(source, "source\nbut");
    let later_break = range_of(source, "source\nand");
    expect!(hard_wrap_breaks(&view).contains(&(old_break.location + 6)), "{:?}", hard_wrap_breaks(&view));
    expect!(view.perform_source_edit(NSRange::new(range_of(source, "Title").upper_bound(), 0), " grows", "Edit"));
    let delta = 6;
    let breaks = hard_wrap_breaks(&view);
    expect!(breaks.contains(&(old_break.location + 6 + delta)), "{breaks:?}");
    expect!(breaks.contains(&(later_break.location + 6 + delta)), "{breaks:?}");
}

fn transient_edit_projection_keeps_touched_hard_wrap_block(mtm: MainThreadMarker) {
    let source = "# Title\n\nFirst prose line wraps in the source\nbut remains one rendered paragraph.\n\nSecond prose line also wraps in the source\nand stays rendered while the first changes.";
    let view = view_for(source, RenderMode::Live, mtm);
    let first_break = range_of(source, "source\nbut");
    let second_break = range_of(source, "source\nand");
    let edit = range_of(source, "First");
    expect!(view.perform_source_edit(NSRange::new(edit.upper_bound(), 0), " edited", "Edit"));
    let delta = 7;
    let breaks = hard_wrap_breaks(&view);
    expect!(breaks.contains(&(first_break.location + 6 + delta)), "{breaks:?}");
    expect!(breaks.contains(&(second_break.location + 6 + delta)), "{breaks:?}");
}

fn transient_structural_edit_drops_touched_hard_wrap_block(mtm: MainThreadMarker) {
    let source = "# Title\n\nFirst prose line wraps in the source\nbut remains one rendered paragraph.\n\nSecond prose line also wraps in the source\nand stays rendered while the first changes.";
    let view = view_for(source, RenderMode::Live, mtm);
    let first_break = range_of(source, "source\nbut");
    let second_break = range_of(source, "source\nand");
    let edit = range_of(source, "First");
    expect!(view.perform_source_edit(NSRange::new(edit.upper_bound(), 0), "\n", "Edit"));
    let breaks = hard_wrap_breaks(&view);
    expect!(!breaks.contains(&(first_break.location + 6)), "{breaks:?}");
    expect!(breaks.contains(&(second_break.location + 7)), "{breaks:?}");
}

fn literal_contexts_bypass_transforms(_mtm: MainThreadMarker) {
    let html = MarkdownPastePayload::Html("<p><strong>literal</strong></p>".into(), "literal".into());
    for source in ["```swift\nlet value = 1\n```\n", "$$\nx + y\n$$\n", "---\ntitle: Draft\n---\n\nBody\n"] {
        let document = parse(source);
        let offset = source.find('\n').map_or(0, |byte| source[..byte].chars().count() as isize);
        let context = MarkdownSmartPaste::context(NSRange::new(offset, 0), &document, RenderMode::Live);
        expect!(context == MarkdownPasteContext::Plain || context == MarkdownPasteContext::Code, "{source:?} → {context:?}");
        expect!(MarkdownSmartPaste::replacement(&html, "", context, MarkdownPasteMode::Smart) == "literal");
    }
}

fn inline_literal_boundaries(_mtm: MainThreadMarker) {
    let source = "Before `code` and $x + y$ after";
    let document = parse(source);
    let code_start = range_of(source, "`code`").location;
    let math_start = range_of(source, "$x + y$").location;
    let html = MarkdownPastePayload::Html("<strong>changed</strong>".into(), "changed".into());
    let context = |offset: isize| MarkdownSmartPaste::context(NSRange::new(offset, 0), &document, RenderMode::Live);
    expect!(context(code_start + 2) == MarkdownPasteContext::Plain);
    expect!(context(code_start - 1) == MarkdownPasteContext::Markdown);
    expect!(context(math_start + 2) == MarkdownPasteContext::Plain);
    expect!(context(math_start - 1) == MarkdownPasteContext::Markdown);
    expect!(MarkdownSmartPaste::replacement(&html, "", MarkdownPasteContext::Plain, MarkdownPasteMode::Smart) == "changed");
}

fn source_mode_passthrough(mtm: MainThreadMarker) {
    let view = view_for("# Source", RenderMode::Source, mtm);
    let payloads = [
        MarkdownPastePayload::Url("www.example.com".into()),
        MarkdownPastePayload::Html("<p><strong>raw</strong></p>".into(), String::new()),
        MarkdownPastePayload::Text("a\tb\n1\t2".into()),
    ];
    for payload in payloads {
        let context = MarkdownSmartPaste::context(NSRange::new(0, 0), &view.parsed_document(), RenderMode::Source);
        let replacement = MarkdownSmartPaste::replacement(&payload, "", context, MarkdownPasteMode::Smart);
        let expected = match &payload {
            MarkdownPastePayload::Url(value) | MarkdownPastePayload::Text(value) => value.clone(),
            MarkdownPastePayload::Html(value, _) => value.clone(),
            _ => unreachable!(),
        };
        expect!(replacement == expected, "{payload:?} → {replacement:?}");
    }
}
