//! Port of `SpeechAccessibilityTests.swift`. The `DensityGutterView` half of
//! `rendererAccessibilitySurface` moves with the density gutter port.

use std::cell::Cell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAccessibility, NSAccessibilityElement, NSAccessibilityElementProtocol};
use objc2_foundation::NSString;
use upleft_core::NSRange;
use upleft_render::engine::render_metrics;
use upleft_render::render_contracts::RenderMode;
use upleft_render::view::gutter_rail_view::GutterRailView;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("speech_rendered_text_and_source_mapping", rendered_text_and_source_mapping),
    ("speech_renderer_accessibility_surface", renderer_accessibility_surface),
    ("speech_heading_rail_hit_is_bound_to_chip", heading_rail_hit_is_bound_to_chip),
    ("speech_rendered_fragment_children", rendered_fragment_children),
    ("speech_front_matter_accessibility_action", front_matter_accessibility_action),
];

fn rendered_text_and_source_mapping(mtm: MainThreadMarker) {
    let source = "Read **this** now.";
    let (view, _storage) = view_with(source, rect(0.0, 0.0, 640.0, 300.0), mtm);
    view.set_mode(RenderMode::Read);
    view.update(parse(source), &wholesale(), true);

    let whole = NSRange::new(0, utf16_len(source));
    let spoken = view.rendered_string_for_speech(whole);
    expect!(spoken == "Read this now.", "spoke {spoken:?}");

    let word = range_of(&spoken, "this");
    expect!(view.source_range_for_speech_range(word, whole) == Some(NSRange::new(7, 4)));
}

fn action_names(names: Option<objc2::rc::Retained<objc2_foundation::NSArray<objc2_app_kit::NSAccessibilityCustomAction>>>) -> Vec<String> {
    names.map(|actions| actions.iter().map(|action| action.name().to_string()).collect()).unwrap_or_default()
}

fn renderer_accessibility_surface(mtm: MainThreadMarker) {
    let (view, _storage) = view_with("# Heading\n", rect(0.0, 0.0, 640.0, 300.0), mtm);
    let rail = GutterRailView::new(&view, mtm);

    expect!(rail.accessibilityRole().map(|role| role.to_string()) == Some("AXGroup".to_owned()));
    expect!(rail.accessibilityLabel().map(|label| label.to_string()) == Some("Document margin".to_owned()));
    expect!(
        action_names(rail.accessibilityCustomActions())
            == vec!["Choose current heading level".to_owned(), "Toggle current section fold".to_owned()]
    );
    expect!(view.accessibilityRole().map(|role| role.to_string()) == Some("AXTextArea".to_owned()));
    expect!(
        action_names(view.accessibilityCustomActions())
            == vec!["Open link at caret".to_owned(), "Copy code block".to_owned()]
    );
}

fn heading_rail_hit_is_bound_to_chip(mtm: MainThreadMarker) {
    let text = "# Heading\n\nBody\n";
    let (view, storage) = view_with(text, rect(0.0, 0.0, 640.0, 300.0), mtm);
    view.set_mode(RenderMode::Read);
    view.update(parse(&storage_string(&storage)), &wholesale(), true);
    let rail = GutterRailView::new(&view, mtm);
    rail.setFrame(rect(0.0, 0.0, render_metrics::GUTTER_WIDTH, 300.0));
    rail.reload();

    view.set_source_selected_ranges(&[NSRange::new(0, 0)]);
    let bounds = rail.bounds();
    let mid_x = bounds.origin.x + bounds.size.width / 2.0;
    expect!(rail.heading_index_at(objc2_foundation::NSPoint::new(mid_x, 8.0)).is_none());
}

fn accessibility_elements(view: &MarkdownTextView) -> Vec<Retained<NSAccessibilityElement>> {
    let children = unsafe { view.accessibilityChildren() };
    children
        .map(|children| {
            children
                .iter()
                .filter_map(|child| child.downcast::<NSAccessibilityElement>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn rendered_fragment_children(mtm: MainThreadMarker) {
    let source = "| A | B |\n| - | - |\n| 1 | 2 |\n\n$$x^2$$\n\n```mermaid\ngraph LR; A-->B\n```";
    let (view, _storage) = view_with(source, rect(0.0, 0.0, 640.0, 480.0), mtm);
    view.update(parse(source), &wholesale(), true);

    let labels: Vec<String> = accessibility_elements(&view)
        .iter()
        .filter_map(|element| element.accessibilityLabel().map(|label| label.to_string()))
        .collect();
    expect!(labels.contains(&"Markdown table".to_owned()), "labels {labels:?}");
    expect!(labels.contains(&"Display math".to_owned()), "labels {labels:?}");
    expect!(labels.contains(&"Mermaid diagram".to_owned()), "labels {labels:?}");
}

struct FrontMatterActivationProbe {
    activations: Cell<usize>,
}

impl MarkdownTextViewDelegate for FrontMatterActivationProbe {
    fn did_activate_front_matter_at(&self, _view: &MarkdownTextView, _range: NSRange) {
        self.activations.set(self.activations.get() + 1);
    }
}

fn front_matter_accessibility_action(mtm: MainThreadMarker) {
    let source = "---\ntitle: Draft\nauthor: Ezzy\n---\n\n# Body\n";
    let (view, _storage) = view_with(source, rect(0.0, 0.0, 640.0, 480.0), mtm);
    let delegate = Rc::new(FrontMatterActivationProbe { activations: Cell::new(0) });
    let as_delegate: Rc<dyn MarkdownTextViewDelegate> = delegate.clone();
    view.set_markdown_delegate(Some(Rc::downgrade(&as_delegate)));
    view.update(parse(source), &wholesale(), true);

    let action = accessibility_elements(&view)
        .into_iter()
        .find(|element| element.accessibilityLabel().map(|label| label.to_string()) == Some("Edit document metadata".to_owned()))
        .expect("an Edit document metadata element");
    expect!(action.accessibilityRole().map(|role| role.to_string()) == Some("AXButton".to_owned()));
    expect!(action.isAccessibilityEnabled());
    expect!(action.accessibilityPerformPress());
    expect!(delegate.activations.get() == 1);
    let _ = NSString::from_str("");
}

#[allow(dead_code)]
fn _traits(_: &dyn NSAccessibility, _: &dyn NSAccessibilityElementProtocol) {}
