//! Copying formulas as TeX from a hosted view (`set_math_copy_as_tex`), an
//! Upleft extension with no Swift counterpart. Every test writes to a
//! private pasteboard made for it, never the general one.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAttributedStringAppKitDocumentFormats, NSPasteboard, NSPasteboardTypeHTML,
    NSPasteboardTypeRTF, NSPasteboardTypeString, NSTextStorage,
};
use objc2_foundation::{NSArray, NSAttributedString, NSString};
use upleft_core::{DirtySet, NSRange};
use upleft_render::render_contracts::Theme;
use upleft_render::theme::style_sheet::{HostTypography, StyleSheet};
use upleft_render::view::markdown_smart_paste::downright_markdown_type;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::expect;
use crate::support::{parse, text_storage, utf16_len};

pub const TESTS: &[crate::Test] = &[
    ("math_copy_writes_tex_in_every_flavour", math_copy_writes_tex_in_every_flavour),
    ("math_copy_of_one_formula_is_that_formula", math_copy_of_one_formula_is_that_formula),
    ("math_copy_takes_a_partly_selected_formula_whole", math_copy_takes_a_partly_selected_formula_whole),
    ("math_copy_off_copies_as_before", math_copy_off_copies_as_before),
    ("math_latex_at_offset_is_the_bare_formula", math_latex_at_offset_is_the_bare_formula),
];

const MESSAGE: &str =
    "Mass–energy: $E = mc^2$ holds.\n\n$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$\n\nAnd a fence:\n\n```math\na^2 + b^2 = c^2\n```\n\nDone.\n";

fn hosted(text: &str, mtm: MainThreadMarker) -> (Retained<MarkdownTextView>, Retained<NSTextStorage>) {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let sheet = Rc::new(StyleSheet::for_host(Theme::fallback(), &appearance, true, HostTypography::default()));
    let storage = text_storage(text);
    let view = MarkdownTextView::new_hosted(&storage, sheet, 480.0, mtm);
    view.update(parse(text), &DirtySet::wholesale(), false);
    (view, storage)
}

/// A pasteboard of our own, released when dropped.
struct PrivatePasteboard(Retained<NSPasteboard>);

impl PrivatePasteboard {
    fn new() -> PrivatePasteboard {
        PrivatePasteboard(NSPasteboard::pasteboardWithUniqueName())
    }
}

impl Drop for PrivatePasteboard {
    fn drop(&mut self) {
        // SAFETY: `releaseGlobally` takes no arguments and returns nothing.
        let _: () = unsafe { msg_send![&*self.0, releaseGlobally] };
    }
}

struct Flavours {
    plain: String,
    rtf: String,
    html: String,
    markdown: String,
}

fn range_of(text: &str, needle: &str, end: &str) -> NSRange {
    let start = text.find(needle).expect("needle");
    let stop = start + text[start..].find(end).expect("end") + end.len();
    NSRange::new(utf16_len(&text[..start]), utf16_len(&text[start..stop]))
}

/// Selects `range` and writes it the way Copy and a drag do.
fn copy(view: &MarkdownTextView, range: NSRange) -> Flavours {
    view.set_source_selected_ranges(&[range]);
    let pasteboard = PrivatePasteboard::new();
    let types = unsafe { NSArray::from_slice(&[NSPasteboardTypeString, NSPasteboardTypeRTF, NSPasteboardTypeHTML]) };
    let wrote: bool = unsafe { msg_send![view, writeSelectionToPasteboard: &*pasteboard.0, types: &*types] };
    expect!(wrote);
    let string = |kind: &NSString| pasteboard.0.stringForType(kind).map(|s| s.to_string()).unwrap_or_default();
    let rtf = pasteboard
        .0
        .dataForType(unsafe { NSPasteboardTypeRTF })
        .and_then(|data| unsafe {
            NSAttributedString::initWithRTF_documentAttributes(NSAttributedString::alloc(), &data, None)
        })
        .map(|attributed| attributed.string().to_string())
        .unwrap_or_default();
    Flavours {
        plain: string(unsafe { NSPasteboardTypeString }),
        rtf,
        html: string(unsafe { NSPasteboardTypeHTML }),
        markdown: string(&downright_markdown_type()),
    }
}

fn math_copy_writes_tex_in_every_flavour(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    view.set_math_copy_as_tex(true);
    let copied = copy(&view, NSRange::new(0, utf16_len(MESSAGE)));
    // Plain text: prose as shown, each formula as TeX, in place.
    expect!(copied.plain.contains("Mass–energy: $E = mc^2$ holds."));
    expect!(copied.plain.contains("$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$"));
    expect!(copied.plain.contains("$$\na^2 + b^2 = c^2\n$$"));
    expect!(copied.plain.contains("Done."));
    expect!(!copied.plain.contains('\u{FFFC}'));
    expect!(!copied.plain.contains("```"));
    // RTF carries the same text.
    expect!(copied.rtf == copied.plain);
    // HTML and the private Markdown flavour come from the source: the TeX
    // travels as text.
    expect!(copied.html.contains("<p>Mass–energy: $E = mc^2$ holds.</p>"));
    expect!(copied.html.contains("<p>$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$</p>"));
    expect!(copied.html.contains("<p>$$\na^2 + b^2 = c^2\n$$</p>"));
    expect!(copied.markdown == MESSAGE.trim_end_matches('\n') || copied.markdown == MESSAGE);
}

fn math_copy_of_one_formula_is_that_formula(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    view.set_math_copy_as_tex(true);
    let inline = copy(&view, range_of(MESSAGE, "$E", "2$"));
    expect!(inline.plain == "$E = mc^2$");
    expect!(inline.rtf == "$E = mc^2$");
    expect!(inline.markdown == "$E = mc^2$");
    expect!(inline.html == "<p>$E = mc^2$</p>");
    let display = copy(&view, range_of(MESSAGE, "$$", "12\n$$"));
    expect!(display.plain == "$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$");
    let fence = copy(&view, range_of(MESSAGE, "```math", "c^2\n```"));
    expect!(fence.plain == "$$\na^2 + b^2 = c^2\n$$");
    expect!(fence.markdown.contains("```math"));
}

fn math_copy_takes_a_partly_selected_formula_whole(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    view.set_math_copy_as_tex(true);
    // From inside the inline formula to the end of its sentence.
    let copied = copy(&view, range_of(MESSAGE, "mc^2", "holds."));
    expect!(copied.plain == "$E = mc^2$ holds.");
    expect!(copied.markdown == "$E = mc^2$ holds.");
    // From the prose before to inside the display formula.
    let copied = copy(&view, range_of(MESSAGE, "holds.", "\\int"));
    expect!(copied.plain.starts_with("holds."));
    expect!(copied.plain.ends_with("$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$"));
    // Prose alone copies as it always has.
    let prose = range_of(MESSAGE, "And", "fence:");
    let copied = copy(&view, prose);
    expect!(copied.plain == view.exportable_attributed_string(prose).string().to_string());
    expect!(copied.plain == "And a fence:");
}

fn math_copy_off_copies_as_before(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    expect!(!view.math_copy_as_tex());
    let range = range_of(MESSAGE, "Mass", "holds.");
    let copied = copy(&view, range);
    // The typeset image's attachment character, as Downright copies it.
    expect!(copied.plain == view.exportable_attributed_string(range).string().to_string());
    expect!(!copied.plain.contains("$E"));
}

fn math_latex_at_offset_is_the_bare_formula(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    let inside = |needle: &str| utf16_len(&MESSAGE[..MESSAGE.find(needle).expect("needle")]);
    expect!(view.math_latex_at_source_offset(inside("mc^2")).as_deref() == Some("E = mc^2"));
    expect!(view.math_latex_at_source_offset(inside("\\tfrac")).as_deref() == Some("\\int_0^1 x\\,dx = \\tfrac12"));
    expect!(view.math_latex_at_source_offset(inside("b^2")).as_deref() == Some("a^2 + b^2 = c^2"));
    expect!(view.math_latex_at_source_offset(inside("Done")).is_none());
}
