//! Port of `CodeBlockGeometryTests.swift`: the code band and its chrome are
//! the most pixel-visible piece of the renderer, so their geometry is locked
//! down here — flush with the column, on the bleed lane, inside its own
//! frame, and on the list or quote content edge when nested.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSLineBreakMode, NSParagraphStyle};
use objc2_core_foundation::{CGPoint, CGRect};
use upleft_core::NSRange;
use upleft_render::appkit_compat::{RectExt, attribute_value, keys};
use upleft_render::engine::render_metrics;
use upleft_render::fragments::code_block_fragment::CodeBlockFragment;
use upleft_render::view::fragment_provider::CodeBlockRole as Role;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("code_block_top_level_band_flushes_with_column", top_level_band_flushes_with_column),
    ("code_block_prose_keeps_the_measure", prose_keeps_the_measure),
    ("code_block_code_wraps_on_word_boundaries", code_wraps_on_word_boundaries),
    ("code_block_nested_code_keeps_chrome_and_list_indent", nested_code_keeps_chrome_and_list_indent),
    ("code_block_nested_code_in_blockquote_keeps_chrome", nested_code_in_blockquote_keeps_chrome),
    ("code_block_deep_nesting_keeps_band_on_content_edge", deep_nesting_keeps_band_on_content_edge),
    ("code_block_closing_band_stays_inside_its_frame", closing_band_stays_inside_its_frame),
    ("code_block_chrome_bands_are_flush_with_their_frames", chrome_bands_are_flush_with_their_frames),
];

struct CodeProbe {
    #[allow(dead_code)]
    source: NSRange,
    role: Role,
    band: CGRect,
    head_indent: f64,
}

/// `headIndent` here is the block's first-line indent.
fn code_fragments(container: &MarkdownContainerView) -> Vec<CodeProbe> {
    fragments_of_class(container.text_view(), "CodeBlockFragment")
        .iter()
        .map(|code| {
            let behavior = code.behavior().as_any().downcast_ref::<CodeBlockFragment>().expect("code behaviour");
            // TextKit calls `draw(at:in:)` in rendering-surface coordinates.
            let surface = code.renderingSurfaceBounds();
            let draw_point = CGPoint::new(-surface.min_x(), -surface.min_y());
            CodeProbe {
                source: code.element_source_range(),
                role: behavior.role,
                band: behavior.band_rect(code, draw_point),
                head_indent: code.paragraph_style().map_or(-1.0, |style| style.firstLineHeadIndent()),
            }
        })
        .collect()
}

fn paragraph_style_at(container: &MarkdownContainerView, offset: isize) -> Option<objc2::rc::Retained<NSParagraphStyle>> {
    let storage = unsafe { container.text_view().textStorage() }?;
    attribute_value(&storage, keys::paragraph_style(), offset as usize)?.downcast::<NSParagraphStyle>().ok()
}

fn top_level_band_flushes_with_column(mtm: MainThreadMarker) {
    let container = read_container("# Top\n\n```swift\nfunc top() {}\n```\n", mtm);
    let fragments = code_fragments(&container);
    let column = container.text_view().column_width();
    expect!(
        (column - (container.text_view().style_sheet().measure_width + render_metrics::CODE_BLEED)).abs() < 0.5,
        "the column should be the measure plus one bleed lane"
    );
    for role in [Role::OpenChrome, Role::Body, Role::CloseChrome] {
        let fragment = fragments.iter().find(|probe| probe.role == role);
        expect!(fragment.is_some(), "missing {role:?} fragment");
        let Some(fragment) = fragment else { continue };
        expect!(fragment.band.min_x().abs() < 0.5, "band starts right of the column");
        expect!((fragment.band.max_x() - column).abs() < 0.5, "band does not reach the bleed lane");
    }
}

fn prose_keeps_the_measure(mtm: MainThreadMarker) {
    let text = "Some prose.\n\n```swift\nfunc top() {}\n```\n";
    let container = read_container(text, mtm);
    let prose = paragraph_style_at(&container, 0);
    let tail = prose.as_ref().map_or(0.0, |style| style.tailIndent());
    expect!((tail + render_metrics::CODE_BLEED).abs() < 0.5, "prose tail indent {tail} does not hold it to the measure");
    let code_start = range_of(text, "func top").location;
    let code = paragraph_style_at(&container, code_start);
    let code_tail = code.as_ref().map_or(0.0, |style| style.tailIndent());
    expect!(
        (code_tail + render_metrics::CODE_INSET_X).abs() < 0.5,
        "code should stop one inset short of the column, not one bleed lane"
    );
    expect!(container.text_view().style_sheet().measure_width > 0.0);
}

fn code_wraps_on_word_boundaries(mtm: MainThreadMarker) {
    let text = "```swift\nfunc decorate(_ storage: NSTextStorage, document: ParsedDocument) {}\n```\n";
    let container = read_container(text, mtm);
    let code_start = range_of(text, "func decorate").location;
    let style = paragraph_style_at(&container, code_start);
    expect!(
        style.as_ref().map(|style| style.lineBreakMode()) == Some(NSLineBreakMode::ByWordWrapping),
        "code still breaks mid-token"
    );
    expect!(
        style.as_ref().map_or(0.0, |style| style.headIndent()) > style.as_ref().map_or(0.0, |style| style.firstLineHeadIndent()),
        "wrapped code rows do not hang"
    );
}

fn nested_code_keeps_chrome_and_list_indent(mtm: MainThreadMarker) {
    let container = read_container("- Item:\n\n  ```python\n  def nested():\n      pass\n  ```\n", mtm);
    let fragments = code_fragments(&container);
    let column = container.text_view().column_width();
    let open: Vec<&CodeProbe> = fragments.iter().filter(|probe| probe.role == Role::OpenChrome).collect();
    expect!(!open.is_empty(), "nested opening fence never became chrome");
    let body_indent = fragments.iter().find(|probe| probe.role == Role::Body).map_or(0.0, |probe| probe.head_indent);
    expect!(body_indent > render_metrics::CODE_INSET_X + 8.0, "nested code lost its list indent");
    if let Some(open) = open.first() {
        expect!(open.band.min_x() > 1.0, "nested band should indent with its list");
        expect!((open.band.max_x() - column).abs() < 0.5, "nested band does not end on the column");
    }
}

fn nested_code_in_blockquote_keeps_chrome(mtm: MainThreadMarker) {
    let container = read_container("> ```swift\n> func quoted() {}\n> ```\n", mtm);
    let fragments = code_fragments(&container);
    for role in [Role::OpenChrome, Role::Body, Role::CloseChrome] {
        expect!(fragments.iter().any(|probe| probe.role == role), "missing {role:?} fragment inside a blockquote");
    }
}

fn deep_nesting_keeps_band_on_content_edge(mtm: MainThreadMarker) {
    let container = read_container("- One\n  - Two\n\n    ```python\n    def deep():\n        pass\n    ```\n", mtm);
    let fragments = code_fragments(&container);
    let column = container.text_view().column_width();
    let open = fragments.iter().find(|probe| probe.role == Role::OpenChrome);
    expect!(open.is_some(), "depth-2 opening fence never became chrome");
    let Some(open) = open else { return };
    let body_indent = fragments.iter().find(|probe| probe.role == Role::Body).map_or(0.0, |probe| probe.head_indent);
    expect!(
        (open.band.min_x() - (body_indent - render_metrics::CODE_INSET_X)).abs() < 0.5,
        "band {} diverged from content edge {}",
        open.band.min_x(),
        body_indent - render_metrics::CODE_INSET_X
    );
    expect!((open.band.max_x() - column).abs() < 0.5, "depth-2 band does not end on the column");
}

fn closing_band_stays_inside_its_frame(mtm: MainThreadMarker) {
    let container = read_container("# Top\n\n```swift\nfunc a() {}\n```\n\nAfter.\n", mtm);
    let fragments = code_fragments(&container);
    let close = fragments.iter().find(|probe| probe.role == Role::CloseChrome).expect("missing closeChrome fragment");
    expect!(
        (close.band.height() - render_metrics::CODE_INSET_Y).abs() < 0.5,
        "closing band height {} is not codeInsetY — it overhangs its frame",
        close.band.height()
    );
}

fn chrome_bands_are_flush_with_their_frames(mtm: MainThreadMarker) {
    let container = read_container("# Top\n\n```swift\nfunc a() {}\n```\n", mtm);
    let fragments = code_fragments(&container);
    let open = fragments.iter().find(|probe| probe.role == Role::OpenChrome);
    let close = fragments.iter().find(|probe| probe.role == Role::CloseChrome);
    expect!(open.is_some() && close.is_some());
    if let Some(open) = open {
        expect!(
            (open.band.height() - render_metrics::CODE_HEADER_HEIGHT).abs() < 0.5,
            "header band should be exactly codeHeaderHeight, no downward overdraw"
        );
    }
    if let Some(close) = close {
        expect!(
            (close.band.height() - render_metrics::CODE_INSET_Y).abs() < 0.5,
            "footer band should be exactly codeInsetY, no upward overdraw"
        );
    }
}
