//! Port of `CalloutGeometryTests.swift`: the callout band is drawn under
//! real glyphs, so its left edge must sit a full icon column left of the
//! glyph edge on every slice, in the draw-time convention (`point` is zero,
//! the origin is the glyph edge).

use objc2::MainThreadMarker;
use objc2_core_foundation::CGPoint;
use upleft_core::model::{BlockContent, MDBlock};
use upleft_core::parser::MarkdownParser;
use upleft_render::appkit_compat::RectExt;
use upleft_render::engine::block_style::{BlockContext, BlockStyleFactory};
use upleft_render::engine::render_metrics;
use upleft_render::fragments::callout_fragment::band_rect;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("callout_body_text_clears_the_rule", body_text_clears_the_rule),
    ("callout_plain_quote_uses_its_own_inset", plain_quote_uses_its_own_inset),
    ("callout_nested_content_keeps_one_band_edge", nested_content_keeps_one_band_edge),
    ("callout_and_quote_keep_separate_indents", callout_and_quote_keep_separate_indents),
];

const ZERO: CGPoint = CGPoint { x: 0.0, y: 0.0 };

fn body_text_clears_the_rule(mtm: MainThreadMarker) {
    let text = "> [!NOTE] Heads up\n> Agents emit these constantly, so they get a real treatment.\n";
    let container = read_container(text, mtm);
    let fragments = fragments_of_class(container.text_view(), "CalloutFragment");
    expect!(!fragments.is_empty(), "no callout fragments were built");
    let inset = render_metrics::CALLOUT_ICON_INSET_X;
    for fragment in &fragments {
        let band = band_rect(fragment, ZERO, inset);
        expect!(
            (band.min_x() + inset).abs() < 0.5,
            "band starts at {}, expected {} — text would sit on the rule",
            band.min_x(),
            -inset
        );
    }
}

fn plain_quote_uses_its_own_inset(mtm: MainThreadMarker) {
    let container = read_container("> Just a quote, no kind marker at all.\n", mtm);
    let fragments = fragments_of_class(container.text_view(), "CalloutFragment");
    expect!(!fragments.is_empty(), "no quote fragments were built");
    let inset = render_metrics::CALLOUT_INSET_X;
    for fragment in &fragments {
        let band = band_rect(fragment, ZERO, inset);
        expect!((band.min_x() + inset).abs() < 0.5, "quote band starts at {}, expected {}", band.min_x(), -inset);
    }
}

fn nested_content_keeps_one_band_edge(mtm: MainThreadMarker) {
    let text = "> [!TIP] With a list\n> Intro line.\n>\n> - first\n> - second\n";
    let container = read_container(text, mtm);
    let fragments = fragments_of_class(container.text_view(), "CalloutFragment");
    expect!(fragments.len() > 1, "expected several slices of one callout");
    let inset = render_metrics::CALLOUT_ICON_INSET_X;
    let edges: Vec<f64> = fragments
        .iter()
        .map(|fragment| band_rect(fragment, ZERO, inset).min_x() + fragment.layoutFragmentFrame().origin.x)
        .collect();
    let first = edges[0];
    for edge in &edges {
        expect!((edge - first).abs() < 0.5, "band edges disagree across slices: {edges:?}");
    }
}

/// `calloutKind` must be part of the paragraph-style cache key.
fn callout_and_quote_keep_separate_indents(_mtm: MainThreadMarker) {
    let sheet = MarkdownTextView::fallback_style_sheet();
    let mut factory = BlockStyleFactory::new(&sheet);
    let document = MarkdownParser::parse("> plain quote\n\n> [!NOTE] kind\n> body\n");

    let mut quote_body: Option<f64> = None;
    let mut callout_body: Option<f64> = None;
    fn walk(
        block: &MDBlock,
        context: BlockContext,
        factory: &mut BlockStyleFactory,
        quote_body: &mut Option<f64>,
        callout_body: &mut Option<f64>,
    ) {
        let mut child = context;
        match &block.content {
            BlockContent::List { .. } => child.list_depth += 1,
            BlockContent::BlockQuote => {
                child.quote_depth += 1;
                child.callout_kind = None;
            }
            BlockContent::Callout { kind, .. } => {
                child.quote_depth += 1;
                child.callout_kind = Some(*kind);
            }
            BlockContent::ListItem { ordinal, .. } => child.ordinal = *ordinal,
            _ => {}
        }
        if matches!(block.content, BlockContent::Paragraph) && context.quote_depth > 0 {
            let indent = factory.paragraph_style(block, context).headIndent();
            if context.callout_kind.is_none() {
                *quote_body = Some(indent);
            } else {
                *callout_body = Some(indent);
            }
        }
        for c in &block.children {
            walk(c, child, factory, quote_body, callout_body);
        }
    }
    walk(&document.root, BlockContext::ROOT, &mut factory, &mut quote_body, &mut callout_body);

    expect!(quote_body.is_some() && callout_body.is_some(), "did not reach both bodies");
    expect!(quote_body != callout_body, "quote and callout bodies both got {quote_body:?} — the cache key aliases them");
    expect!(callout_body == Some(render_metrics::CALLOUT_ICON_INSET_X), "callout body indent {callout_body:?} is not the icon column");
}
