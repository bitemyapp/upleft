//! The Rust counterparts of `DensityHost`, `DensityHoverScene` and
//! `DensityModelDump` in `oracle/Sources/downright-oracle/DensityDump.swift`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSApplication, NSBitmapImageRep, NSControl, NSEvent, NSEventModifierFlags, NSEventType,
    NSLayoutAttribute, NSTableView, NSTextField, NSView, NSWindow,
};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::{CGColor, CGColorSpace};
use objc2_foundation::{NSPoint, NSRect};
use serde_json::Value;
use upleft_core::metrics::Metrics;
use upleft_core::parser::MarkdownParser;
use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::{ChangeKind, NSRange, ParsedDocument};
use upleft_render::appkit_compat::{RectExt, main_after};
use upleft_render::render_contracts::{RenderMode, SourceFocus};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::density_gutter_view::{
    DensityBand, DensityBandKind, DensityGutterDelegate, DensityGutterView, Pip, Selection,
};
use upleft_render::view::density_outline_window::{DensityOutlineEntry, DensityOutlineWindow};
use upleft_render::view::markdown_container_view::MarkdownContainerView;

use super::attribute_dump::{color_json, font_json};
use super::json::{Object, double};
use super::render::MarkdownScene;
use super::style_sheet::appearance_named;
use super::{Failure, Request};
use crate::capture::{CaptureRequest, CaptureScene};

/// `DensityHost`: a `DensityGutterView` in a `MarkdownContainerView`, set up
/// the way Downright's `DocumentWindowController` sets it up.
pub struct DensityHost {
    container: Retained<MarkdownContainerView>,
    gutter: Retained<DensityGutterView>,
    text: String,
    document: RefCell<Arc<ParsedDocument>>,
    requested_fractions: RefCell<Vec<CGFloat>>,
}

impl DensityHost {
    pub fn new(
        container: &MarkdownContainerView,
        side: &str,
        style_sheet: Rc<StyleSheet>,
        text: &str,
        mtm: MainThreadMarker,
    ) -> Rc<DensityHost> {
        // `let densityGutterView = DensityGutterView()`.
        let gutter = DensityGutterView::new_current(mtm);
        let accessory: Retained<NSView> = Retained::into_super(Retained::into_super(gutter.clone()));
        if side == "trailing" {
            container.set_trailing_accessory(Some(accessory));
        } else {
            container.set_leading_accessory(Some(accessory));
        }
        let host = Rc::new(DensityHost {
            container: container.retain_container(),
            gutter: gutter.clone(),
            text: text.to_owned(),
            document: RefCell::new(MarkdownParser::parse("")),
            requested_fractions: RefCell::new(Vec::new()),
        });
        let weak: Weak<dyn DensityGutterDelegate> = Rc::downgrade(&(host.clone() as Rc<dyn DensityGutterDelegate>));
        gutter.set_delegate(Some(weak));
        gutter.set_style_sheet(style_sheet);
        host
    }

    pub fn gutter(&self) -> &Retained<DensityGutterView> {
        &self.gutter
    }

    pub fn requested_fractions(&self) -> Vec<CGFloat> {
        self.requested_fractions.borrow().clone()
    }

    /// `DocumentWindowController.refreshDensityBands(metrics:)`.
    pub fn refresh_density_bands(&self, parsed: Arc<ParsedDocument>) {
        *self.document.borrow_mut() = parsed.clone();
        let metrics = Metrics::section_metrics(&parsed);
        let mut word_count: isize = metrics.iter().map(|metric| metric.words).sum();
        if word_count == 0 && parsed.length > 0 {
            word_count = whitespace_split_count(&self.text);
        }
        let read_minutes = 1isize.max((word_count + 199) / 200);
        self.gutter.set_metrics_summary(format!("{word_count} words · {read_minutes} min read"));
        self.gutter.set_bands(DensityGutterView::bands_for(&parsed, &[], &[]));
        let length = 1isize.max(parsed.length) as CGFloat;
        let current = self.visible_heading_index(self.container.text_view().top_visible_offset());
        self.gutter.set_outline_entries(
            parsed
                .headings
                .iter()
                .enumerate()
                .map(|(index, heading)| {
                    DensityOutlineEntry::new(
                        heading.title.clone(),
                        heading.level,
                        heading.range.location as CGFloat / length,
                        Some(index as isize) == current,
                    )
                })
                .collect(),
        );
        self.gutter.setNeedsDisplay(true);
    }

    /// The gutter half of `updateBreadcrumbAndGutter()`.
    pub fn update_gutter(&self) {
        let text_view = self.container.text_view();
        let current = self.visible_heading_index(text_view.top_visible_offset());
        let length = 1isize.max(self.document.borrow().length);
        let top = text_view.top_visible_offset() as CGFloat / length as CGFloat;
        let scroll_view = self.container.scroll_view();
        let visible_height = scroll_view.contentView().bounds().height();
        let document_height = swift_max(1.0, scroll_view.documentView().map_or(1.0, |view| view.bounds().height()));
        let span = swift_min(1.0, visible_height / document_height);
        self.gutter.set_visible_range((top, swift_min(1.0, top + span)));
        self.gutter.set_read_progress(swift_max(self.gutter.read_progress(), swift_min(1.0, top + span)));
        let entries = self.gutter.outline_entries();
        let previous_current = entries.iter().position(|entry| entry.is_current).map(|index| index as isize);
        if previous_current != current {
            self.gutter.set_outline_entries(
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, mut entry)| {
                        entry.is_current = Some(index as isize) == current;
                        entry
                    })
                    .collect(),
            );
        }
    }

    pub fn visible_heading_index(&self, offset: isize) -> Option<isize> {
        let document = self.document.borrow();
        let headings = &document.headings;
        let mut low = 0usize;
        let mut high = headings.len();
        while low < high {
            let middle = (low + high) / 2;
            if headings[middle].range.location <= offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low > 0 { Some(low as isize - 1) } else { None }
    }
}

impl DensityGutterDelegate for DensityHost {
    fn density_gutter_did_request_scroll_to_fraction(&self, _gutter: &DensityGutterView, fraction: CGFloat) {
        self.requested_fractions.borrow_mut().push(fraction);
    }

    fn density_gutter_preview_at_fraction(
        &self,
        gutter: &DensityGutterView,
        fraction: CGFloat,
    ) -> Option<(String, String, String)> {
        let document = self.document.borrow().clone();
        let offset = (fraction * document.length as CGFloat) as isize;
        let Some(index) = document.headings.iter().rposition(|heading| heading.range.location <= offset) else {
            return Some(("Document start".to_owned(), String::new(), gutter.metrics_summary()));
        };
        let heading = &document.headings[index];
        let section_position = format!("Section {} of {}", index + 1, document.headings.len());
        let context = if heading.word_count > 0 {
            format!("{section_position} · {} words", heading.word_count)
        } else {
            section_position
        };
        let text_view = self.container.text_view();
        if text_view.mode() == RenderMode::Source || text_view.source_focus() == SourceFocus::Document {
            let snippet_length = 160isize.min(0isize.max(document.length - offset));
            return Some((heading.title.clone(), document.substring(NSRange::new(offset, snippet_length)), context));
        }
        Some((
            heading.title.clone(),
            StructuralZoom::section_preview(&document, index as isize).unwrap_or_else(|| "Section overview".to_owned()),
            context,
        ))
    }
}

/// `text.split(whereSeparator: { $0.isWhitespace }).count`.
fn whitespace_split_count(text: &str) -> isize {
    let mut count = 0;
    let mut in_word = false;
    for grapheme in upleft_swift_text::graphemes(text) {
        if upleft_swift_text::is_whitespace(grapheme) {
            in_word = false;
        } else if !in_word {
            in_word = true;
            count += 1;
        }
    }
    count
}

fn swift_max(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

fn swift_min(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

trait RetainContainer {
    fn retain_container(&self) -> Retained<MarkdownContainerView>;
}

impl RetainContainer for MarkdownContainerView {
    fn retain_container(&self) -> Retained<MarkdownContainerView> {
        use objc2::Message;
        self.retain()
    }
}

// MARK: - density-hover

/// `DensityHoverScene`.
pub struct DensityHoverScene {
    markdown: MarkdownScene,
    action: String,
    window: Option<Retained<NSWindow>>,
    ready_at: Rc<Cell<Option<Instant>>>,
}

impl DensityHoverScene {
    const DRIVE_DELAY: f64 = 0.3;
    const SETTLE_AFTER_DRIVE: f64 = 1.5;

    pub fn new(markdown: MarkdownScene, action: String) -> DensityHoverScene {
        DensityHoverScene { markdown, action, window: None, ready_at: Rc::new(Cell::new(None)) }
    }
}

fn drive(gutter: &DensityGutterView, window: &NSWindow, action: &str) {
    if action == "outline" {
        gutter.present_outline_for_keyboard();
        return;
    }
    let share: CGFloat = action.parse::<f64>().unwrap_or(0.5);
    let positions = gutter.mark_positions_for_testing();
    if positions.is_empty() {
        return;
    }
    let index = ((positions.len() - 1) as CGFloat * share).round() as usize;
    let local = NSPoint::new(gutter.bounds().mid_x(), positions[index]);
    let location = gutter.convertPoint_toView(local, None);
    let Some(event) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::MouseMoved,
        location,
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        0,
        0,
        0.0,
    ) else {
        return;
    };
    let _: () = unsafe { msg_send![gutter, mouseEntered: &*event] };
    let _: () = unsafe { msg_send![gutter, mouseMoved: &*event] };
}

impl CaptureScene for DensityHoverScene {
    fn build(&mut self, window: &NSWindow, request: &CaptureRequest, mtm: MainThreadMarker) -> Result<Retained<NSView>, String> {
        use objc2::Message;
        self.window = Some(window.retain());
        self.markdown.build(window, request, mtm)
    }

    fn after_show(&mut self, window: &NSWindow) {
        self.markdown.after_show(window);
        let gutter = self.markdown.density_host().map(|host| host.gutter().clone());
        let window = self.window.clone();
        let action = self.action.clone();
        let ready_at = self.ready_at.clone();
        main_after(Self::DRIVE_DELAY, move || {
            if let (Some(gutter), Some(window)) = (&gutter, &window) {
                drive(gutter, window, &action);
            }
            ready_at.set(Some(Instant::now() + Duration::from_secs_f64(Self::SETTLE_AFTER_DRIVE)));
        });
    }

    fn before_settle_check(&mut self) {
        self.markdown.before_settle_check();
    }

    fn write_extras(&mut self, _bitmap: &NSBitmapImageRep, _request: &CaptureRequest) -> Result<(), String> {
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.ready_at.get().is_some_and(|ready_at| Instant::now() >= ready_at)
    }

    fn extra_windows(&self) -> Vec<Retained<NSWindow>> {
        let Some(window) = &self.window else { return Vec::new() };
        window
            .childWindows()
            .map(|children| children.iter().filter(|child| child.isVisible()).collect())
            .unwrap_or_default()
    }
}

// MARK: - density-model

const HEIGHTS: [CGFloat; 4] = [100.0, 300.0, 700.0, 1400.0];

/// `DensityModelDump.overlays(_:)`.
fn overlays(document: &ParsedDocument) -> (Vec<(ChangeKind, NSRange)>, Vec<NSRange>) {
    let kinds = [ChangeKind::Inserted, ChangeKind::Modified, ChangeKind::Deleted];
    let mut changes = Vec::new();
    let mut hits = Vec::new();
    for (index, block) in document.root.children.iter().enumerate() {
        if index % 4 == 1 {
            changes.push((kinds[(index / 4) % 3], block.range));
        }
        if index % 5 == 2 {
            hits.push(NSRange::new(block.range.location, 3isize.min(block.range.length)));
        }
    }
    (changes, hits)
}

/// `density-model`.
pub fn model(request: &Request) -> Result<(), Failure> {
    let text = super::markup::read_text(&request.input).map_err(|error| Failure::Error(format!("{error:?}")))?;
    let mtm = MainThreadMarker::new().expect("density-model runs on the main thread");
    let _ = NSApplication::sharedApplication(mtm);
    let document = MarkdownParser::parse(&text);
    let (changes, hits) = overlays(&document);
    let bands = DensityGutterView::bands_for(&document, &changes, &hits);
    let plain = DensityGutterView::bands_for(&document, &[], &[]);
    let appearance: Retained<NSAppearance> = appearance_named(request.dark);
    let themes = ThemeStore::shared().themes();
    let Some(theme) = themes.iter().find(|theme| theme.name == request.theme).cloned() else {
        let known: Vec<String> = themes.iter().map(|theme| theme.name.clone()).collect();
        return Err(Failure::Error(format!("unknown theme {}; known: {}", request.theme, known.join(", "))));
    };
    let calm = Rc::new(StyleSheet::new(theme.clone(), &appearance, Some(true)));
    let lively = Rc::new(StyleSheet::new(theme, &appearance, Some(false)));
    let value = Object::new()
        .with("length", document.length as i64)
        .with("bands", Value::Array(bands.iter().map(band).collect()))
        .with("plainBands", plain.len() as i64)
        .with("tracks", Value::Array(HEIGHTS.iter().map(|height| track(*height, &bands)).collect()))
        .with(
            "views",
            Value::Array(vec![view(700.0, &bands, calm.clone(), mtm), view(300.0, &bands, calm.clone(), mtm)]),
        )
        .with("springs", springs(700.0, &plain, lively, mtm))
        .with("outline", outline(&document, calm, mtm))
        .build();
    Ok(super::json::write(&value, &request.output)?)
}

fn kind(kind: DensityBandKind) -> Value {
    Value::String(match kind {
        DensityBandKind::Heading { level } => format!("heading{level}"),
        DensityBandKind::CodeBlock => "codeBlock".to_owned(),
        DensityBandKind::Table => "table".to_owned(),
        DensityBandKind::Math => "math".to_owned(),
        DensityBandKind::TaskList => "taskList".to_owned(),
        DensityBandKind::Change(change) => format!("change.{}", change.raw_value()),
        DensityBandKind::SearchHit => "searchHit".to_owned(),
        DensityBandKind::Image => "image".to_owned(),
        DensityBandKind::Callout => "callout".to_owned(),
    })
}

fn band(band: &DensityBand) -> Value {
    Value::Array(vec![kind(band.kind), double(band.start_fraction), double(band.end_fraction)])
}

fn pip(pip: &Pip) -> Value {
    Value::Array(vec![
        Value::String(pip.change.map_or(String::new(), |change| change.raw_value().to_owned())),
        Value::Bool(pip.search_hit),
    ])
}

fn selection(selection: &Selection) -> Value {
    Object::new()
        .with("marks", Value::Array(selection.marks.iter().map(band).collect()))
        .with("pips", Value::Array(selection.pips.iter().map(pip).collect()))
        .build()
}

fn doubles(values: &[CGFloat]) -> Value {
    Value::Array(values.iter().map(|value| double(*value)).collect())
}

fn track(height: CGFloat, bands: &[DensityBand]) -> Value {
    let track = DensityGutterView::track_range(height, 28.0);
    let track_height = track.1 - track.0;
    let capacity = DensityGutterView::stack_capacity(track_height);
    let selected = DensityGutterView::selection_for(bands, capacity, true);
    let plain = DensityGutterView::selection_for(bands, capacity, false);
    let count = selected.marks.len() as isize;
    let pitch = DensityGutterView::mark_pitch(track_height, count);
    let positions_with = |pointer: Option<CGFloat>| {
        DensityGutterView::centered_band_y_positions(
            height,
            count,
            28.0,
            pitch,
            pointer,
            DensityGutterView::STACK_COMPRESSION,
            DensityGutterView::PROXIMITY_RADIUS,
        )
    };
    let positions = positions_with(None);
    let mut pointers = vec![height * 0.25, height * 0.5, height * 0.75];
    pointers.extend(positions.iter().take(3));
    let compressed: Vec<Value> = pointers.iter().map(|pointer| doubles(&positions_with(Some(*pointer)))).collect();
    let current: Vec<Value> = [(0.0, 0.1), (0.3, 0.5), (0.9, 1.0)]
        .iter()
        .map(|range| DensityGutterView::current_heading_fraction_in(&selected.marks, *range).map_or(Value::Null, double))
        .collect();
    let slop = DensityGutterView::dismissal_slop(&positions);
    let mut sweep = Vec::new();
    let mut previous: Option<isize> = None;
    let mut y: CGFloat = 0.0;
    while y <= height {
        let next = DensityGutterView::next_hovered_band_index(
            y,
            &positions,
            previous,
            DensityGutterView::HOVER_ACTIVATION_SLOP,
            slop,
        );
        sweep.push(Value::from(next.unwrap_or(-1) as i64));
        previous = next;
        y += 7.0;
    }
    let influence: Vec<CGFloat> = (0..=8)
        .map(|step| DensityGutterView::proximity_influence(f64::from(step) * 5.0, DensityGutterView::PROXIMITY_RADIUS))
        .collect();
    Object::new()
        .with("height", double(height))
        .with("track", doubles(&[track.0, track.1]))
        .with("capacity", capacity as i64)
        .with("selection", selection(&selected))
        .with("plain", selection(&plain))
        .with("pitch", double(pitch))
        .with("positions", doubles(&positions))
        .with("compressed", Value::Array(compressed))
        .with("current", Value::Array(current))
        .with("dismissalSlop", double(slop))
        .with("sweep", Value::Array(sweep))
        .with("influence", doubles(&influence))
        .build()
}

fn cg_color(color: Option<&CGColor>) -> Value {
    let Some(color) = color else { return Value::Null };
    let space = CGColor::color_space(Some(color))
        .and_then(|space| CGColorSpace::name(Some(&space)))
        .map(|name| name.to_string())
        .unwrap_or_default();
    let count = CGColor::number_of_components(Some(color));
    let pointer = CGColor::components(Some(color));
    let components: Vec<CGFloat> =
        if pointer.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(pointer, count) }.to_vec() };
    Object::new().with("space", space).with("components", doubles(&components)).build()
}

fn rect_json(rect: NSRect) -> Value {
    Value::Array(vec![double(rect.origin.x), double(rect.origin.y), double(rect.size.width), double(rect.size.height)])
}

fn layers(view: &NSView) -> Value {
    let sublayers = view.layer().and_then(|layer| unsafe { layer.sublayers() });
    Value::Array(
        sublayers
            .map(|sublayers| {
                sublayers
                    .iter()
                    .map(|layer| {
                        Object::new()
                            .with("frame", rect_json(layer.frame()))
                            .with("cornerRadius", double(layer.cornerRadius()))
                            .with("hidden", layer.isHidden())
                            .with("background", cg_color(layer.backgroundColor().as_deref()))
                            .with("shadowOpacity", double(f64::from(layer.shadowOpacity())))
                            .with("shadowRadius", double(layer.shadowRadius()))
                            .with("shadowColor", cg_color(layer.shadowColor().as_deref()))
                            .build()
                    })
                    .collect()
            })
            .unwrap_or_default(),
    )
}

fn event(kind: NSEventType, y: CGFloat, gutter: &DensityGutterView) -> Retained<NSEvent> {
    let location = gutter.convertPoint_toView(NSPoint::new(gutter.bounds().mid_x(), y), None);
    NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind,
        location,
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        1,
        1.0,
    )
    .expect("a mouse event")
}

fn send(gutter: &DensityGutterView, selector: &str, event: &NSEvent) {
    unsafe {
        match selector {
            "mouseDown" => msg_send![gutter, mouseDown: event],
            "mouseUp" => msg_send![gutter, mouseUp: event],
            "mouseDragged" => msg_send![gutter, mouseDragged: event],
            "mouseEntered" => msg_send![gutter, mouseEntered: event],
            "mouseMoved" => msg_send![gutter, mouseMoved: event],
            "mouseExited" => msg_send![gutter, mouseExited: event],
            _ => unreachable!(),
        }
    }
}

/// `DensityModelDump.Recorder`.
struct Recorder {
    fractions: RefCell<Vec<CGFloat>>,
}

impl DensityGutterDelegate for Recorder {
    fn density_gutter_did_request_scroll_to_fraction(&self, _gutter: &DensityGutterView, fraction: CGFloat) {
        self.fractions.borrow_mut().push(fraction);
    }

    fn density_gutter_preview_at_fraction(&self, _gutter: &DensityGutterView, _fraction: CGFloat) -> Option<(String, String, String)> {
        None
    }
}

fn make_gutter(
    height: CGFloat,
    bands: &[DensityBand],
    style_sheet: Rc<StyleSheet>,
    recorder: &Rc<Recorder>,
    mtm: MainThreadMarker,
) -> Retained<DensityGutterView> {
    let gutter = DensityGutterView::new(style_sheet, mtm);
    gutter.set_perform_haptic_feedback(Rc::new(|| {}));
    let weak: Weak<dyn DensityGutterDelegate> = Rc::downgrade(&(recorder.clone() as Rc<dyn DensityGutterDelegate>));
    gutter.set_delegate(Some(weak));
    gutter.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), objc2_foundation::NSSize::new(DensityGutterView::WIDTH, height)));
    gutter.set_bands(bands.to_vec());
    gutter.layoutSubtreeIfNeeded();
    gutter
}

fn view(height: CGFloat, bands: &[DensityBand], style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Value {
    let recorder = Rc::new(Recorder { fractions: RefCell::new(Vec::new()) });
    let gutter = make_gutter(height, bands, style_sheet, &recorder, mtm);
    let mut states = Vec::new();
    let record = |states: &mut Vec<Value>, name: String| {
        states.push(Object::new().with("state", name).with("layers", layers(&gutter)).build());
    };
    record(&mut states, "rest".into());
    gutter.set_visible_range((0.0, 0.2));
    gutter.set_read_progress(0.2);
    record(&mut states, "top".into());
    gutter.set_shows_overlay_pips(true);
    record(&mut states, "pips".into());
    let positions = gutter.mark_positions_for_testing();
    let mut targets = vec![height / 2.0];
    if let (Some(first), Some(last)) = (positions.first(), positions.last()) {
        targets.extend([*first, positions[positions.len() / 2], *last, first + 5.0]);
    }
    for (index, y) in targets.iter().enumerate() {
        gutter.drive_hover_for_testing(*y);
        record(&mut states, format!("hover{index}"));
    }
    gutter.set_visible_range((0.5, 0.7));
    gutter.set_read_progress(0.7);
    record(&mut states, "scrolled".into());

    let mut hits = Vec::new();
    let mut samples = vec![0.0, 10.0, height / 4.0, height / 2.0, height * 3.0 / 4.0, height - 10.0];
    for y in positions.iter().take(4) {
        samples.extend([y - 3.0, y + 3.0]);
    }
    for y in samples {
        send(&gutter, "mouseDown", &event(NSEventType::LeftMouseDown, y, &gutter));
        send(&gutter, "mouseUp", &event(NSEventType::LeftMouseUp, y, &gutter));
        let click = recorder.fractions.borrow().last().copied();
        send(&gutter, "mouseDown", &event(NSEventType::LeftMouseDown, y, &gutter));
        send(&gutter, "mouseDragged", &event(NSEventType::LeftMouseDragged, y + 12.0, &gutter));
        send(&gutter, "mouseUp", &event(NSEventType::LeftMouseUp, y + 12.0, &gutter));
        let drag = recorder.fractions.borrow().last().copied();
        hits.push(Value::Array(vec![double(y), click.map_or(Value::Null, double), drag.map_or(Value::Null, double)]));
    }
    send(&gutter, "mouseExited", &event(NSEventType::MouseMoved, -40.0, &gutter));
    record(&mut states, "exited".into());
    let value_description = gutter.accessibilityValueDescription().map(|value| value.to_string()).unwrap_or_default();
    Object::new()
        .with("height", double(height))
        .with("positions", doubles(&positions))
        .with("states", Value::Array(states))
        .with("hits", Value::Array(hits))
        .with("requested", doubles(&recorder.fractions.borrow()))
        .with("valueDescription", value_description)
        .build()
}

fn spring_tick(gutter: &DensityGutterView, dt: CGFloat) -> bool {
    unsafe { msg_send![gutter, springTick: dt] }
}

fn spring_apply(gutter: &DensityGutterView) {
    let _: () = unsafe { msg_send![gutter, springApply] };
}

fn springs(height: CGFloat, bands: &[DensityBand], style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Value {
    let recorder = Rc::new(Recorder { fractions: RefCell::new(Vec::new()) });
    let gutter = make_gutter(height, bands, style_sheet, &recorder, mtm);
    let positions = gutter.mark_positions_for_testing();
    let Some(first) = positions.first().copied() else { return Value::Null };
    let target = positions[positions.len() / 2];
    let mut frames = Vec::new();
    send(&gutter, "mouseEntered", &event(NSEventType::MouseMoved, first, &gutter));
    send(&gutter, "mouseMoved", &event(NSEventType::MouseMoved, target, &gutter));
    for frame in 1..=6 {
        let moving = spring_tick(&gutter, 1.0 / 120.0);
        spring_apply(&gutter);
        if frame % 2 == 0 {
            frames.push(Object::new().with("moving", moving).with("layers", layers(&gutter)).build());
        }
    }
    let mut ticks: i64 = 0;
    while spring_tick(&gutter, 1.0 / 120.0) && ticks < 600 {
        ticks += 1;
    }
    spring_apply(&gutter);
    frames.push(Object::new().with("settledAfter", ticks).with("layers", layers(&gutter)).build());
    send(&gutter, "mouseExited", &event(NSEventType::MouseMoved, -40.0, &gutter));
    for _ in 1..=3 {
        let _ = spring_tick(&gutter, 1.0 / 60.0);
    }
    spring_apply(&gutter);
    frames.push(Object::new().with("exiting", true).with("layers", layers(&gutter)).build());
    Value::Array(frames)
}

fn outline(document: &ParsedDocument, style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Value {
    let length = 1isize.max(document.length) as CGFloat;
    let mut low = 0usize;
    let mut high = document.headings.len();
    while low < high {
        let middle = (low + high) / 2;
        if document.headings[middle].range.location <= 0 {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let current = if low > 0 { Some(low - 1) } else { None };
    let entries: Vec<DensityOutlineEntry> = document
        .headings
        .iter()
        .enumerate()
        .map(|(index, heading)| {
            DensityOutlineEntry::new(
                heading.title.clone(),
                heading.level,
                heading.range.location as CGFloat / length,
                Some(index) == current,
            )
        })
        .collect();
    let window = DensityOutlineWindow::new(style_sheet, mtm);
    window.set_entries(entries.clone());
    let table = NSTableView::new(mtm);
    let mut rows = Vec::new();
    for row in 0..entries.len().min(40) {
        let cell: Option<Retained<NSView>> = unsafe {
            msg_send![&*window, tableView: &*table, viewForTableColumn: None::<&AnyObject>, row: row as isize]
        };
        let Some(cell) = cell else {
            rows.push(Value::Null);
            continue;
        };
        let label = cell.subviews().iter().find_map(|view| view.downcast::<NSTextField>().ok());
        let leading = cell.constraints().iter().find(|constraint| {
            let first = unsafe { constraint.firstItem() };
            first.is_some_and(|first| {
                label.as_ref().is_some_and(|label| std::ptr::eq(&*first as *const AnyObject, &**label as *const NSTextField as *const AnyObject))
            }) && constraint.firstAttribute() == NSLayoutAttribute::Leading
        });
        let title = label.as_ref().map(|label| label.stringValue().to_string()).unwrap_or_default();
        let font = label.as_ref().and_then(|label| label.font()).map_or(Value::Null, |font| font_json(&font));
        let color = label.as_ref().and_then(|label| label.textColor()).map_or(Value::Null, |color| color_json(&color));
        let line_break_mode = label.as_ref().map_or(0, |label| NSControl::lineBreakMode(label).0 as i64);
        let corner_radius = cell.layer().map_or(-1.0, |layer| layer.cornerRadius());
        let background = cg_color(cell.layer().and_then(|layer| layer.backgroundColor()).as_deref());
        rows.push(
            Object::new()
                .with("title", title)
                .with("font", font)
                .with("color", color)
                .with("lineBreakMode", line_break_mode)
                .with("leading", leading.map_or(Value::Null, |constraint| double(constraint.constant())))
                .with("cornerRadius", double(corner_radius))
                .with("background", background)
                .build(),
        );
    }
    let row_count: isize = unsafe { msg_send![&*window, numberOfRowsInTableView: &*table] };
    Object::new()
        .with(
            "entries",
            Value::Array(
                entries
                    .iter()
                    .map(|entry| {
                        Value::Array(vec![
                            Value::String(entry.title.clone()),
                            Value::from(entry.level as i64),
                            double(entry.fraction),
                            Value::Bool(entry.is_current),
                        ])
                    })
                    .collect(),
            ),
        )
        .with("rows", row_count as i64)
        .with("cells", Value::Array(rows))
        .build()
}

#[allow(dead_code)]
fn _in_scope(_: &dyn NSAccessibility) {}
