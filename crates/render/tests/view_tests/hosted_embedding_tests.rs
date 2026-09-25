//! Hosted embedding (docs/EMBEDDING.md), an Upleft extension with no Swift
//! counterpart: a view sized to its content inside a host's own scroll view,
//! streamed into, never scrolling anything.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSScrollView, NSTextStorage, NSView};
use objc2_foundation::{NSDictionary, NSObjectProtocol, NSSize, NSString};
use upleft_core::ast_diff::ASTDiff;
use upleft_core::{DirtySet, NSRange, ParsedDocument};
use upleft_render::engine::block_style::{BlockContext, BlockStyleFactory};
use upleft_render::render_contracts::{FragmentKind, FragmentPayload, Theme, attribute_keys};
use upleft_render::theme::style_sheet::{BodyFamily, HostTypography, StyleSheet};
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::{MarkdownTextViewDelegate, ScrollPosition};

use crate::expect;
use crate::support::{parse, rect, text_storage, utf16_len};

pub const TESTS: &[crate::Test] = &[
    ("hosted_height_is_the_laid_out_content", hosted_height_is_the_laid_out_content),
    ("hosted_height_reports_synchronously", hosted_height_reports_synchronously),
    ("hosted_width_sets_the_container_width", hosted_width_sets_the_container_width),
    ("hosted_view_never_scrolls_its_host", hosted_view_never_scrolls_its_host),
    ("open_fence_renders_as_code_while_streaming", open_fence_renders_as_code_while_streaming),
    ("streamed_message_matches_whole_message", streamed_message_matches_whole_message),
    ("host_style_sheet_typography", host_style_sheet_typography),
    ("streamed_message_cut_back_matches_its_head", streamed_message_cut_back_matches_its_head),
    ("laid_out_again_matches_a_fresh_layout", laid_out_again_matches_a_fresh_layout),
    ("table_rows_added_after_layout_share_the_table", table_rows_added_after_layout_share_the_table),
    ("hosted_lines_are_found_again", hosted_lines_are_found_again),
];

fn host_sheet(typography: HostTypography) -> Rc<StyleSheet> {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    Rc::new(StyleSheet::for_host(Theme::fallback(), &appearance, true, typography))
}

fn hosted(text: &str, width: f64, mtm: MainThreadMarker) -> (Retained<MarkdownTextView>, Retained<NSTextStorage>) {
    let storage = text_storage(text);
    let view = MarkdownTextView::new_hosted(&storage, host_sheet(HostTypography::default()), width, mtm);
    view.update(parse(text), &DirtySet::wholesale(), false);
    (view, storage)
}

/// Appends `piece` the way a host must: edit, parse, diff, update, in one
/// main-thread turn.
fn append(view: &MarkdownTextView, storage: &NSTextStorage, piece: &str) {
    let previous = view.parsed_document();
    let length = storage.length();
    storage.beginEditing();
    storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(length, 0), &NSString::from_str(piece));
    storage.endEditing();
    let fresh = parse(&storage.string().to_string());
    let dirty = ASTDiff::dirty_set(Some(&previous), &fresh);
    view.update(fresh, &dirty, true);
}

struct HeightRecorder {
    reports: RefCell<Vec<f64>>,
}

impl MarkdownTextViewDelegate for HeightRecorder {
    fn did_change_content_height(&self, _view: &MarkdownTextView, height: f64) {
        self.reports.borrow_mut().push(height);
    }
}

fn hosted_height_is_the_laid_out_content(mtm: MainThreadMarker) {
    let text = "# Title\n\nA paragraph long enough to wrap onto a second line at this width, which it should.\n\n- one\n- two\n";
    let (view, _storage) = hosted(text, 320.0, mtm);
    let layout = view.textLayoutManager().expect("TextKit 2");
    let used = layout.usageBoundsForTextContainer();
    expect!(used.size.height > 0.0);
    // No overscroll, no viewport minimum: the frame is the content.
    expect!(view.frame().size.height == used.origin.y + used.size.height);
    expect!(view.content_height() == view.frame().size.height);
    expect!(view.frame().size.width == 320.0);
    let before = view.frame().size.height;
    view.set_hosted_insets(NSSize::new(10.0, 6.0));
    expect!(view.frame().size.height == before + 12.0);
    expect!(view.frame().size.width == 340.0);
    // An empty message is as tall as its insets.
    let (empty, _storage) = hosted("", 320.0, mtm);
    expect!(empty.frame().size.height == 0.0);
}

fn hosted_height_reports_synchronously(mtm: MainThreadMarker) {
    let (view, storage) = hosted("First paragraph.", 400.0, mtm);
    let recorder = Rc::new(HeightRecorder { reports: RefCell::new(Vec::new()) });
    let delegate: Rc<dyn MarkdownTextViewDelegate> = recorder.clone();
    view.set_markdown_delegate(Some(Rc::downgrade(&delegate)));
    let before = view.frame().size.height;
    append(&view, &storage, "\n\nA second paragraph.");
    // Reported inside `update`, before it returned, with the new frame.
    let reports = recorder.reports.borrow().clone();
    expect!(reports.len() == 1);
    expect!(reports[0] > before);
    expect!(reports[0] == view.frame().size.height);
    // An append that changes nothing visible reports nothing.
    append(&view, &storage, "");
    expect!(recorder.reports.borrow().len() == 1);
}

fn hosted_width_sets_the_container_width(mtm: MainThreadMarker) {
    let text = "Words ".repeat(60);
    let (view, _storage) = hosted(&text, 600.0, mtm);
    let recorder = Rc::new(HeightRecorder { reports: RefCell::new(Vec::new()) });
    let delegate: Rc<dyn MarkdownTextViewDelegate> = recorder.clone();
    view.set_markdown_delegate(Some(Rc::downgrade(&delegate)));
    let wide = view.frame().size.height;
    view.set_hosted_width(300.0);
    let container = unsafe { view.textContainer() }.expect("container");
    expect!(container.size().width == 300.0);
    expect!(view.frame().size.width == 300.0);
    expect!(view.frame().size.height > wide);
    expect!(recorder.reports.borrow().last() == Some(&view.frame().size.height));
}

fn hosted_view_never_scrolls_its_host(mtm: MainThreadMarker) {
    let text = (1..=80).map(|n| format!("Paragraph {n}.\n\n")).collect::<String>();
    let (view, _storage) = hosted(&text, 400.0, mtm);
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(0.0, 0.0, 420.0, 300.0));
    let stack = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 420.0, view.frame().size.height + 40.0));
    view.setFrameOrigin(objc2_foundation::NSPoint::new(10.0, 20.0));
    stack.addSubview(&view);
    scroll.setDocumentView(Some(&stack));
    let clip = scroll.contentView();
    let origin = clip.bounds().origin;
    let end = utf16_len(&text) - 2;
    view.scroll_to_offset(end, ScrollPosition::Center, false);
    let _: () = unsafe { msg_send![&*view, scrollRangeToVisible: objc2_foundation::NSRange::new(end as usize, 1)] };
    view.update(parse(&text), &DirtySet::wholesale(), true);
    view.prepare_for_display();
    expect!(clip.bounds().origin == origin);
    // And a link click never reaches NSWorkspace.
    let link = NSString::from_str("https://example.com");
    let _: () = unsafe { msg_send![&*view, clickedOnLink: &*link as &AnyObject, atIndex: 0usize] };
}

fn payload_kind_at(view: &MarkdownTextView, offset: isize) -> Option<FragmentKind> {
    let storage = unsafe { view.textStorage() }?;
    let value: Retained<AnyObject> = unsafe {
        storage.attribute_atIndex_effectiveRange(attribute_keys::dr_fragment(), offset as usize, std::ptr::null_mut())
    }?;
    value.downcast::<FragmentPayload>().ok().map(|payload| payload.kind())
}

fn open_fence_renders_as_code_while_streaming(mtm: MainThreadMarker) {
    let (view, storage) = hosted("", 500.0, mtm);
    view.set_streaming(true);
    for piece in ["Before.\n\n", "```mermaid\n", "flowchart LR\n", "    A --> B\n"] {
        append(&view, &storage, piece);
    }
    let fence = utf16_len("Before.\n\n") + 2;
    expect!(payload_kind_at(&view, fence) == Some(FragmentKind::CodeBlock));
    // The stream ends without closing the fence: it is what it says it is.
    view.set_streaming(false);
    expect!(payload_kind_at(&view, fence) == Some(FragmentKind::Mermaid));
    // Streaming again, a closed fence is a diagram at once.
    view.set_streaming(true);
    append(&view, &storage, "```\n");
    expect!(payload_kind_at(&view, fence) == Some(FragmentKind::Mermaid));
    view.set_streaming(false);
}

/// Every attribute run of a view's storage, with object identities (payloads,
/// block identities, attachments) reduced to their presence.
fn attribute_runs(view: &MarkdownTextView) -> Vec<(usize, usize, Vec<String>)> {
    let storage = unsafe { view.textStorage() }.expect("storage");
    let mut runs = Vec::new();
    let mut index = 0usize;
    while index < storage.length() {
        let mut range = objc2_foundation::NSRange::new(0, 0);
        let attributes: Retained<NSDictionary<NSString, AnyObject>> =
            unsafe { storage.attributesAtIndex_effectiveRange(index, &mut range) };
        let (keys, values) = attributes.to_vecs();
        let mut entries: Vec<String> = keys
            .iter()
            .zip(values.iter())
            .map(|(key, value)| {
                let key = key.to_string();
                if ["drFragment", "drBlock", "NSAttachment"].contains(&key.as_str()) {
                    return key;
                }
                let description: Retained<NSString> = unsafe { msg_send![&**value, description] };
                format!("{key}={description}")
            })
            .collect();
        entries.sort();
        runs.push((range.location, range.length, entries));
        index = range.location + range.length;
    }
    runs
}

fn display_substitutions(view: &MarkdownTextView) -> Vec<(NSRange, isize, bool, bool, Option<String>)> {
    view.current_display_map()
        .substitutions()
        .iter()
        .map(|sub| {
            (
                sub.source_range,
                sub.display_length,
                sub.is_hidden,
                sub.is_hard_wrap_reflow,
                sub.replacement.as_ref().map(|replacement| replacement.string().to_string()),
            )
        })
        .collect()
}

/// Every layout fragment: class, element offset, height.
fn fragments(view: &MarkdownTextView) -> Vec<(String, isize, f64, f64)> {
    use objc2_app_kit::NSTextSelectionDataSource;
    let layout = view.textLayoutManager().expect("TextKit 2");
    let out = RefCell::new(Vec::new());
    let block = block2::StackBlock::new(|fragment: std::ptr::NonNull<objc2_app_kit::NSTextLayoutFragment>| -> objc2::runtime::Bool {
        let fragment = unsafe { fragment.as_ref() };
        let start = layout.offsetFromLocation_toLocation(&layout.documentRange().location(), &fragment.rangeInElement().location());
        let class = fragment.class().name().to_string_lossy().into_owned();
        let frame = fragment.layoutFragmentFrame();
        out.borrow_mut().push((class, start, frame.size.height, frame.origin.y));
        objc2::runtime::Bool::YES
    });
    layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
        Some(&layout.documentRange().location()),
        objc2_app_kit::NSTextLayoutFragmentEnumerationOptions::EnsuresLayout,
        &block,
    );
    out.into_inner()
}

/// What TextKit lays out: each layout fragment's text, line by line.
fn laid_out_lines(view: &MarkdownTextView) -> Vec<String> {
    use objc2_app_kit::NSTextSelectionDataSource;
    let layout = view.textLayoutManager().expect("TextKit 2");
    let out = RefCell::new(Vec::new());
    let block = block2::StackBlock::new(|fragment: std::ptr::NonNull<objc2_app_kit::NSTextLayoutFragment>| -> objc2::runtime::Bool {
        let fragment = unsafe { fragment.as_ref() };
        let text = fragment
            .textElement()
            .and_then(|element| element.downcast::<objc2_app_kit::NSTextParagraph>().ok())
            .map(|paragraph| paragraph.attributedString().string().to_string())
            .unwrap_or_default();
        let units: Vec<u16> = text.encode_utf16().collect();
        let lines = fragment.textLineFragments();
        for index in 0..lines.count() {
            let range = lines.objectAtIndex(index).characterRange();
            let end = (range.location + range.length).min(units.len());
            out.borrow_mut().push(String::from_utf16_lossy(&units[range.location.min(end)..end]));
        }
        objc2::runtime::Bool::YES
    });
    layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
        Some(&layout.documentRange().location()),
        objc2_app_kit::NSTextLayoutFragmentEnumerationOptions::EnsuresLayout,
        &block,
    );
    out.into_inner()
}

/// Runs the main queue until every diagram and formula has landed.
fn settle() {
    crate::support::pump_main_queue(|| upleft_render::fragments::async_objects::pending_count() == 0, std::time::Duration::from_secs(20));
    crate::support::pump_main_queue(|| false, std::time::Duration::from_millis(50));
}

/// Streams `text` in pieces from a fixed seed (1–`max` characters each).
fn stream(text: &str, max: usize, mtm: MainThreadMarker) -> Retained<MarkdownTextView> {
    let (view, storage) = hosted("", 520.0, mtm);
    view.set_streaming(true);
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let take = (1 + (seed >> 33) as usize % max).min(characters.len() - index);
        let piece: String = characters[index..index + take].iter().collect();
        append(&view, &storage, &piece);
        index += take;
    }
    view.set_streaming(false);
    view
}

/// A message streamed in token-sized pieces ends up exactly as the same
/// message given whole: storage attributes, display map, paragraphs, height.
fn streamed_message_matches_whole_message(mtm: MainThreadMarker) {
    let documents: [&str; 8] = [
        include_str!("fixtures/hosted-stress.md"),
        // A quote whose paragraph goes on in lazy lines, joined as they come.
        "> The batch design made sense when uploads were a few per hour.\nIt stopped making sense around the time\nthe mobile app shipped.\n\nAfter.\n",
        // Inline math, a footnote reference, then more blocks.
        "Where $\\bar{s}$ is the size and $\\mu$ the rate, $\\rho \\approx 0.31$ holds at 32 partitions, so there is room before we need more.[^m]\n\n## Risks\n\n- [ ] **Ordering.** Two uploads.\n\n[^m]: A note.\n",
        // A setext underline and a list marker arrive mid-stream.
        "Title\n===\n\n- aaaa\n  - b\n- [x] A finished task\n\nPara\n---\n",
        // References and footnotes defined after their uses.
        "See [the docs][docs] and a note[^n].\n\nMore [docs] here.\n\n[docs]: https://example.com \"Docs\"\n[^n]: The note.\n",
        // Paired HTML across blocks, a table growing, CRLF line ends.
        "<details>\r\n<summary>More</summary>\r\n\r\nHidden **text**.\r\n\r\n</details>\r\n\r\n| a | b |\r\n| - | - |\r\n| 1 | 2 |\r\n| 3 | 4 |\r\n",
        // A footnote and a reference defined again further on: the first
        // definitions stop being the ones the text resolves, and show.
        "A note.[^x] See [docs].\n\n[^x]: The first note.\n\n[docs]: https://example.com/a\n\nMore text.\n\n[^x]: The second note.\n\n[docs]: https://example.com/b\n",
        // A fence left open for a while, math, a thematic break.
        "```rust\nfn main() {}\n```\n\n$$\nx^2\n$$\n\n***\n\n```math\n\\frac{1}{2}\n```\n",
    ];
    let checked = Cell::new(0);
    for (number, text) in documents.iter().enumerate() {
        for max in [1usize, 8] {
            let streamed = stream(text, max, mtm);
            let (whole, _storage) = hosted(text, 520.0, mtm);
            settle();
            let label = format!("document {number}, pieces of up to {max}");
            let (a, b) = (attribute_runs(&streamed), attribute_runs(&whole));
            if a != b {
                let first = a.iter().zip(&b).position(|(x, y)| x != y);
                if let Some(index) = first {
                    eprintln!("  streamed {:?}\n  whole    {:?}", a[index], b[index]);
                }
                panic!("{label}: attribute runs differ at run {first:?}");
            }
            assert!(display_substitutions(&streamed) == display_substitutions(&whole), "{label}: display maps differ");
            assert!(streamed.paragraph_index() == whole.paragraph_index(), "{label}: paragraph indexes differ");
            let (a, b) = (laid_out_lines(&streamed), laid_out_lines(&whole));
            if a != b {
                let first = a.iter().zip(&b).position(|(x, y)| x != y);
                if let Some(index) = first {
                    eprintln!("  streamed {:?}\n  whole    {:?}", a[index], b[index]);
                }
                panic!("{label}: laid-out lines differ at line {first:?}");
            }
            if streamed.content_height() != whole.content_height() {
                let (fa, fb) = (fragments(&streamed), fragments(&whole));
                for (x, y) in fa.iter().zip(&fb) {
                    if x.3 != y.3 || x.2 != y.2 {
                        eprintln!("  streamed {x:?}\n  whole    {y:?}");
                    }
                }
                panic!("{label}: heights differ: {} vs {} ({} vs {} fragments)", streamed.content_height(), whole.content_height(), fa.len(), fb.len());
            }
            checked.set(checked.get() + 1);
        }
    }
    expect!(checked.get() == 16);
}

/// A streamed message cut back to a head that ends before a top-level
/// block (a host sealing it, the rest going on in another view) ends up as
/// a view of the head alone.
fn streamed_message_cut_back_matches_its_head(mtm: MainThreadMarker) {
    let quote = "Intro.\n\n> The batch design made sense when uploads were a few per hour.\nIt stopped making sense around the time\nthe mobile app shipped.\n\nMiddle paragraph.\n\nThe rest, which goes on.\n";
    let stress = include_str!("fixtures/hosted-stress.md");
    let mut cases: Vec<(&str, usize, usize)> = vec![(quote, quote.find("The rest").expect("the rest"), 1), (quote, quote.find("The rest").expect("the rest"), 8)];
    // Every paragraph after a blank line in the stress fixture, the rest
    // streamed a little way on.
    let mut at = 0;
    while let Some(found) = stress[at..].find("\n\n") {
        let cut = at + found + 2;
        at = cut;
        if stress[cut..].starts_with(|c: char| c.is_alphabetic()) {
            cases.push((stress, cut, 6));
        }
    }
    for (text, cut, max) in cases {
        let rest = (cut + 40).min(text.len());
        let rest = (rest..=text.len()).find(|&end| text.is_char_boundary(end)).unwrap_or(text.len());
        let text = &text[..rest];
        let (streamed, storage) = hosted("", 520.0, mtm);
        streamed.set_streaming(true);
        let characters: Vec<char> = text.chars().collect();
        let mut index = 0;
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        while index < characters.len() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let take = (1 + (seed >> 33) as usize % max).min(characters.len() - index);
            append(&streamed, &storage, &characters[index..index + take].iter().collect::<String>());
            index += take;
        }
        streamed.set_streaming(false);
        let previous = streamed.parsed_document();
        let head = &text[..cut];
        storage.beginEditing();
        let units = head.encode_utf16().count();
        storage.replaceCharactersInRange_withString(
            objc2_foundation::NSRange::new(units, storage.length() - units),
            &NSString::from_str(""),
        );
        storage.endEditing();
        let fresh = parse(head);
        let dirty = ASTDiff::dirty_set(Some(&previous), &fresh);
        streamed.update(fresh, &dirty, true);
        let (whole, _storage) = hosted(head, 520.0, mtm);
        settle();
        let label = format!("cut at {cut}, pieces of up to {max}");
        let (a, b) = (attribute_runs(&streamed), attribute_runs(&whole));
        if a != b {
            let first = a.iter().zip(&b).position(|(x, y)| x != y);
            if let Some(index) = first {
                eprintln!("  cut back {:?}\n  whole    {:?}", a[index], b[index]);
            } else {
                eprintln!("  cut back ends {:?}\n  whole ends    {:?}", &a[a.len().saturating_sub(2)..], &b[b.len().saturating_sub(2)..]);
            }
            panic!("{label}: attribute runs differ at run {first:?}");
        }
        let (a, b) = (display_substitutions(&streamed), display_substitutions(&whole));
        if a != b {
            eprintln!("  cut back {a:?}\n  whole    {b:?}");
            panic!("{label}: display maps differ");
        }
        let (a, b) = (laid_out_lines(&streamed), laid_out_lines(&whole));
        if a != b {
            eprintln!("  cut back {a:?}\n  whole    {b:?}");
            panic!("{label}: laid-out lines differ");
        }
        assert!(streamed.content_height() == whole.content_height(), "{label}: heights differ");
    }
}

fn host_style_sheet_typography(_mtm: MainThreadMarker) {
    let terminal = host_sheet(HostTypography {
        body_family: Some(BodyFamily::Monospaced),
        body_size: Some(14.0),
        heading_sizes: [Some(18.0), Some(16.0), None, None, None, None],
        code_size: Some(13.0),
        line_height_multiple: Some(1.45),
        paragraph_spacing: Some(9.0),
        code_bleed: Some(0.0),
        code_header: None,
        ..HostTypography::default()
    });
    expect!(terminal.revision == 0);
    expect!(terminal.body_font().isFixedPitch());
    expect!(terminal.body_font().pointSize() == 14.0);
    expect!(terminal.heading_font(1).pointSize() == 18.0);
    expect!(terminal.heading_font(2).pointSize() == 16.0);
    expect!(terminal.mono_font(None).pointSize() == 13.0);
    // 14 × 1.45 = 20.3, kept to the half point; the grid divides it.
    expect!(terminal.line_height == 20.5);
    expect!(terminal.baseline_grid * 4.0 == terminal.line_height);
    expect!(terminal.code_bleed() == 0.0);
    let mut factory = BlockStyleFactory::new(&terminal);
    let document: Arc<ParsedDocument> = parse("A paragraph.\n");
    let paragraph = &document.root.children[0];
    let style = factory.paragraph_style(paragraph, BlockContext::ROOT);
    expect!(style.paragraphSpacing() == 9.0);
    expect!(style.tailIndent() == 0.0);

    let editorial = host_sheet(HostTypography {
        body_family: Some(BodyFamily::NewYork),
        body_size: Some(15.5),
        hyphenation_factor: Some(0.9),
        ..HostTypography::default()
    });
    expect!(editorial.body_font().pointSize() == 15.5);
    let style = BlockStyleFactory::new(&editorial).paragraph_style(paragraph, BlockContext::ROOT);
    expect!(style.hyphenationFactor() == 0.9);

    // No host typography: the style sheet a Downright view would get.
    let plain = host_sheet(HostTypography::default());
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let downright = StyleSheet::new(Theme::fallback(), &appearance, Some(true));
    let downright_body = downright.body_font();
    let other: &AnyObject = downright_body.as_ref();
    expect!(plain.body_font().isEqual(Some(other)));
    expect!(plain.line_height == downright.line_height);
    expect!(plain.measure_width == downright.measure_width);
}

/// A paragraph with a footnote's superscript, laid out again (a new width,
/// an invalidation), is as tall as a first layout at that width makes it.
fn laid_out_again_matches_a_fresh_layout(mtm: MainThreadMarker) {
    for text in [
        "A paragraph.[^1]\n\nB paragraph.[^1] More.\n\n[^1]: A note.\n",
        "Where $x$ holds.[^m]\n\n## Next\n\nText.\n\n[^m]: Note.\n",
    ] {
        let (view, _storage) = hosted(text, 520.0, mtm);
        settle();
        view.set_hosted_width(720.0);
        let (fresh, _other) = hosted(text, 720.0, mtm);
        settle();
        expect!(view.content_height() == fresh.content_height());
        view.invalidate_all_fragments();
        expect!(view.content_height() == fresh.content_height());
    }
}

/// Rows appended to a table whose first rows are already laid out measure
/// with them: every row's fragment reads the table as it is now.
fn table_rows_added_after_layout_share_the_table(mtm: MainThreadMarker) {
    let head = "| Check | Query | Threshold |\n| :--- | :--- | ---: |\n| Row counts per tenant | `count(*)` grouped by tenant | 0 |\n";
    let rest = "| Late rows | rows with `ingested_at` more than 60 s after upload | 0.1 % |\n| Duplicates | rows sharing a natural key | 0 |\n\nAfter.\n";
    let (view, storage) = hosted(head, 520.0, mtm);
    settle();
    let _ = view.content_height();
    append(&view, &storage, rest);
    let whole_text = format!("{head}{rest}");
    let (whole, _other) = hosted(&whole_text, 520.0, mtm);
    settle();
    expect!(view.content_height() == whole.content_height());
}

/// A host keeps the reader's place by the line at the top of its viewport:
/// every line `hosted_line_at` names is where `hosted_line_top` finds it,
/// and after the width changes the same character's line is found.
fn hosted_lines_are_found_again(mtm: MainThreadMarker) {
    let text = "# Keeping a place\n\nA paragraph with **strong** and *emphasised* words, `inline code`, and a [link](https://example.com), long enough to wrap onto several lines at this width and more at a narrower one.\n\n- one item\n- another item that is long enough to wrap onto a second line\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n> A quote, also long enough to wrap onto more than one line at the narrower width.\n";
    let (view, _storage) = hosted(text, 320.0, mtm);
    let height = view.frame().size.height;
    let mut lines = Vec::new();
    let mut y = 0.0;
    while y < height {
        let (line, top) = view.hosted_line_at(y).expect("a line");
        expect!(top <= y + 0.001);
        expect!(view.hosted_line_top(line) == Some(top));
        if lines.last().is_none_or(|(last, _)| *last != line) {
            lines.push((line, top));
        }
        y += 3.0;
    }
    expect!(lines.len() > 8);
    expect!(lines.windows(2).all(|pair| pair[0].1 < pair[1].1));
    view.set_hosted_width(200.0);
    for (line, _) in &lines {
        let top = view.hosted_line_top(*line).expect("found again");
        let (found, found_top) = view.hosted_line_at(top + 0.5).expect("a line there");
        expect!(found_top == top);
        expect!(found.paragraph == line.paragraph);
        expect!(found.character <= line.character);
    }
}
