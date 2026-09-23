//! Port of `Tests/MarkdownRenderTests/DensityRailTests.swift`.

use std::cell::Cell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
use objc2_foundation::{NSArray, NSPoint, NSString};
use upleft_core::{ChangeKind, NSRange};
use upleft_render::motion::{self, FrameClock, SpringScalar};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::density_gutter_preview_window::DensityGutterPreviewWindow;
use upleft_render::view::density_gutter_view::{DensityBand, DensityBandKind, DensityGutterView};
use upleft_render::view::density_outline_window::{DensityOutlineEntry, DensityOutlineWindow};

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("density_preview_uses_resolved_appearance", preview_uses_resolved_appearance),
    ("density_outline_geometry_and_timing", outline_geometry_and_timing),
    ("density_preview_width_respects_text_boundary", preview_width_respects_text_boundary),
    ("density_spring_is_frame_rate_independent", spring_is_frame_rate_independent),
    ("density_sub_unit_springs_traverse", sub_unit_springs_traverse),
    ("density_hover_hysteresis", hover_hysteresis),
    ("density_hover_handover", hover_handover),
    ("density_proximity_and_heading_widths", proximity_and_heading_widths),
    ("density_arming_does_not_rebase_the_clock", arming_does_not_rebase_the_clock),
    ("density_re_arming_starts_fresh", re_arming_starts_fresh),
    ("density_neighborhood_dim_policy", neighborhood_dim_policy),
    ("density_stack_span_envelope", stack_span_envelope),
    ("density_capacity_follows_track", capacity_follows_track),
    ("density_stack_compression_near_pointer", stack_compression_near_pointer),
    ("density_at_rest_bands", at_rest_bands),
    ("density_body_bands_are_not_indexed", body_bands_are_not_indexed),
    ("density_outline_entry_state", outline_entry_state),
    ("density_centered_band_positions", centered_band_positions),
    ("density_depth_budget_keeps_structure", depth_budget_keeps_structure),
    ("density_depth_budget_fills_spare_slots", depth_budget_fills_spare_slots),
    ("density_depth_budget_keeps_shallow_documents", depth_budget_keeps_shallow_documents),
    ("density_overlays_do_not_evict_headings", overlays_do_not_evict_headings),
    ("density_overlay_pips_are_opt_in", overlay_pips_are_opt_in),
    ("density_pips_attach_to_enclosing_section", pips_attach_to_enclosing_section),
    ("density_mixed_changes_collapse_to_modified", mixed_changes_collapse_to_modified),
    ("density_sparse_documents_keep_heading_anchors", sparse_documents_keep_heading_anchors),
    ("density_headingless_documents_index_overlays", headingless_documents_index_overlays),
    ("density_stride_sampling_keeps_ends", stride_sampling_keeps_ends),
    ("density_current_heading_policy", current_heading_policy),
    ("density_scrub_activation_has_a_real_threshold", scrub_activation_has_a_real_threshold),
    ("density_detent_fires_on_arrival_and_crossing_only", detent_fires_on_arrival_and_crossing_only),
    ("density_detents_are_rate_limited_across_a_scrub", detents_are_rate_limited_across_a_scrub),
    ("density_reduce_motion_silences_the_rail", reduce_motion_silences_the_rail),
];

fn appearance(dark: bool) -> objc2::rc::Retained<NSAppearance> {
    let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
    NSAppearance::appearanceNamed(name).expect("system appearance")
}

fn best_match(appearance: Option<objc2::rc::Retained<NSAppearance>>) -> Option<String> {
    let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe { NSAppearanceNameDarkAqua }]);
    appearance.and_then(|appearance| appearance.bestMatchFromAppearancesWithNames(&names)).map(|name| name.to_string())
}

fn preview_uses_resolved_appearance(mtm: MainThreadMarker) {
    let window = DensityGutterPreviewWindow::new(
        Rc::new(StyleSheet::new(ThemeStore::shared().current(), &appearance(false), None)),
        mtm,
    );
    expect!(best_match(window.appearance()) == Some("NSAppearanceNameAqua".to_owned()));

    window.set_style_sheet(Rc::new(StyleSheet::new(ThemeStore::shared().current(), &appearance(true), None)));
    expect!(best_match(window.appearance()) == Some("NSAppearanceNameDarkAqua".to_owned()));
    expect!(
        best_match(window.contentView().and_then(|view| view.appearance()))
            == Some("NSAppearanceNameDarkAqua".to_owned())
    );
}

fn outline_geometry_and_timing(_mtm: MainThreadMarker) {
    expect!(DensityGutterView::WIDTH == 72.0);
    expect!(DensityGutterView::HOVER_DWELL == 0.02);
    expect!(DensityGutterView::HOVER_ACTIVATION_SLOP == 22.0);
    expect!(DensityGutterView::HOVER_DISMISSAL_SLOP == 4.0);
    expect!(DensityGutterView::PREVIEW_EXIT_DELAY == 0.06);
    expect!(DensityGutterView::DETENT_INTERVAL == 0.05);
    expect!(DensityGutterView::PROXIMITY_RADIUS == 36.0);
    expect!(DensityGutterView::MAGNETIC_PULL == 1.5);
    expect!(DensityGutterView::SCRUB_VELOCITY_PULL == 2.0);
    expect!(DensityGutterView::STACK_COMPRESSION == 0.08);
    expect!(DensityGutterView::BREATHE_SCALE == 1.08);
    expect!(DensityGutterView::NEIGHBORHOOD_DIM == 0.82);
    expect!(DensityGutterView::JUMP_PUNCH_BOOST == 4.0);
    expect!(motion::SPRING_QUICK == 0.12);
    expect!(motion::SPRING_STANDARD == 0.20);
    expect!(motion::SPRING_DELIBERATE == 0.32);
    expect!(motion::JUMP_PUNCH_KICK == 480.0);
    expect!(motion::BREATHE == motion::QUICK);
    expect!(motion::PREVIEW_STAGGER == motion::QUICK / 3.0);
    expect!(DensityOutlineWindow::ROW_HEIGHT == 44.0);
    expect!(DensityOutlineWindow::CORNER_RADIUS == 14.0);
    expect!(DensityOutlineWindow::SHOW_DWELL == 0.25);
    expect!(DensityOutlineWindow::SHOW_DURATION == 0.12);
    expect!(DensityOutlineWindow::HIDE_DURATION == 0.09);
}

fn preview_width_respects_text_boundary(_mtm: MainThreadMarker) {
    expect!(DensityGutterPreviewWindow::compact_overlay_width(720.0) == 302.4);
    expect!(DensityGutterPreviewWindow::compact_overlay_width(300.0) == 140.0);
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(72.0, Some(412.0), None, false) == Some(320.0));
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(72.0, Some(360.0), None, false) == Some(280.0));
    // A ~1020pt window leaves ~166pt of margin beside the wall-pinned rail.
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(72.0, Some(246.0), None, false) == Some(166.0));
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(72.0, Some(220.0), None, false) == Some(140.0));
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(72.0, Some(200.0), None, false).is_none());
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(800.0, None, None, true) == Some(320.0));
    expect!(DensityGutterPreviewWindow::resolved_maximum_width(-40.0, Some(300.0), Some(4.0), false) == Some(296.0));
}

/// The integrator is the closed-form damped-harmonic solution, so advancing
/// half twice must land exactly where advancing once lands.
fn spring_is_frame_rate_independent(_mtm: MainThreadMarker) {
    for dt in [1.0 / 120.0, 1.0 / 60.0, 1.0 / 30.0] {
        let mut whole = SpringScalar::with_value(0.0, motion::SPRING_QUICK);
        whole.target(100.0);
        let mut split = SpringScalar::with_value(0.0, motion::SPRING_QUICK);
        split.target(100.0);
        let _ = whole.advance(dt);
        let _ = split.advance(dt / 2.0);
        let _ = split.advance(dt / 2.0);
        expect!((whole.value() - split.value()).abs() < 1e-9);
        expect!((whole.velocity() - split.velocity()).abs() < 1e-9);
    }
}

fn sub_unit_springs_traverse(_mtm: MainThreadMarker) {
    let mut glow = SpringScalar::with_value(0.0, motion::SPRING_QUICK);
    glow.target(0.12);
    let _ = glow.advance(1.0 / 120.0);
    expect!(glow.value() > 0.0);
    expect!(glow.value() < 0.12);
}

fn next(y: f64, positions: &[f64], current: Option<isize>, activation: f64, dismissal: f64) -> Option<isize> {
    DensityGutterView::next_hovered_band_index(y, positions, current, activation, dismissal)
}

fn hover_hysteresis(_mtm: MainThreadMarker) {
    let positions = [100.0, 200.0];
    let activation = DensityGutterView::HOVER_ACTIVATION_SLOP;
    let dismissal = DensityGutterView::HOVER_DISMISSAL_SLOP;

    expect!(next(100.0 + activation - 1.0, &positions, None, activation, dismissal) == Some(0));
    expect!(next(100.0 + activation + 1.0, &positions, None, activation, dismissal).is_none());
    expect!(next(100.0 + dismissal, &positions, Some(0), activation, dismissal) == Some(0));
    expect!(next(100.0 + dismissal + 1.0, &positions, Some(0), activation, dismissal).is_none());
    expect!(next(200.0 - activation + 1.0, &positions, Some(0), activation, dismissal) == Some(1));
}

fn hover_handover(_mtm: MainThreadMarker) {
    let tight = [100.0, 109.0, 118.0];
    let wide = [100.0, 120.0, 140.0];
    expect!(DensityGutterView::dismissal_slop(&tight) == 4.5);
    expect!(DensityGutterView::dismissal_slop(&wide) == 10.0);
    expect!(DensityGutterView::dismissal_slop(&[100.0, 104.0]) == 4.0);
    expect!(DensityGutterView::dismissal_slop(&[100.0]) == 4.0);

    let slop = DensityGutterView::dismissal_slop(&wide);
    for step in 0..=40 {
        let y = 100.0 + f64::from(step) / 2.0;
        expect!(next(y, &wide, Some(0), DensityGutterView::HOVER_ACTIVATION_SLOP, slop).is_some());
    }
}

fn proximity_and_heading_widths(_mtm: MainThreadMarker) {
    let radius = DensityGutterView::PROXIMITY_RADIUS;
    expect!(DensityGutterView::proximity_influence(0.0, radius) == 1.0);
    expect!(DensityGutterView::proximity_influence(36.0, radius) == 0.0);
    let mid = DensityGutterView::proximity_influence(18.0, radius);
    expect!(mid > 0.4 && mid < 0.6);

    for level in 1..=4 {
        expect!(DensityGutterView::heading_mark_width(level, false) == 26.0);
        expect!(DensityGutterView::heading_mark_width(level, true) == 32.0);
    }
}

fn arming_does_not_rebase_the_clock(_mtm: MainThreadMarker) {
    let mut clock = FrameClock::default();
    expect!(clock.start(0.0));
    expect!(clock.is_running());

    let frame = 1.0 / 120.0;
    expect!((clock.tick(frame) - frame).abs() < 1e-9);
    expect!(!clock.start(frame * 1.9));
    expect!((clock.tick(frame * 2.0) - frame).abs() < 1e-9);

    clock.stop();
    expect!(!clock.is_running());
    expect!(clock.start(99.0));
}

fn re_arming_starts_fresh(_mtm: MainThreadMarker) {
    let mut clock = FrameClock::default();
    clock.start(100.0);
    let _ = clock.tick(100.5);
    clock.stop();
    clock.start(900.0);
    expect!((clock.tick(900.01) - 0.01).abs() < 1e-9);
}

fn neighborhood_dim_policy(_mtm: MainThreadMarker) {
    expect!(DensityGutterView::neighborhood_factor(2, None) == 1.0);
    expect!(DensityGutterView::neighborhood_factor(2, Some(2)) == 1.0);
    expect!(DensityGutterView::neighborhood_factor(3, Some(2)) == 0.92);
    expect!(DensityGutterView::neighborhood_factor(5, Some(2)) == 0.82);
}

fn stack_span_envelope(_mtm: MainThreadMarker) {
    let mut height = 120.0;
    while height <= 1600.0 {
        let track = DensityGutterView::track_range(height, 28.0);
        let track_height = track.1 - track.0;
        let capacity = DensityGutterView::stack_capacity(track_height);
        if capacity >= DensityGutterView::MINIMUM_STACK_MARKS {
            let pitch = DensityGutterView::mark_pitch(track_height, capacity);
            expect!(pitch >= DensityGutterView::MIN_PITCH);
            expect!(pitch <= DensityGutterView::MAX_PITCH);

            let positions = DensityGutterView::centered_band_y_positions(
                height,
                capacity,
                28.0,
                pitch,
                None,
                DensityGutterView::STACK_COMPRESSION,
                DensityGutterView::PROXIMITY_RADIUS,
            );
            let span = positions.last().unwrap() - positions.first().unwrap();
            expect!(span <= track_height * DensityGutterView::MAX_SPAN_FRACTION + 0.001);
            expect!(span >= track_height * 0.12);
            expect!(*positions.first().unwrap() >= track.0 - 0.001);
            expect!(*positions.last().unwrap() <= track.1 + 0.001);
        }
        height += 20.0;
    }
}

fn capacity_follows_track(_mtm: MainThreadMarker) {
    let tall = DensityGutterView::stack_capacity(1344.0);
    let medium = DensityGutterView::stack_capacity(244.0);
    let short = DensityGutterView::stack_capacity(44.0);
    let tiny = DensityGutterView::stack_capacity(20.0);

    expect!(tall == DensityGutterView::STACK_CAPACITY_CEILING);
    expect!(medium > short);
    expect!(short > tiny);
    expect!(tiny < DensityGutterView::MINIMUM_STACK_MARKS);

    let mut track = 40.0;
    while track <= 1600.0 {
        let capacity = DensityGutterView::stack_capacity(track);
        if capacity > 1 {
            let pitch = DensityGutterView::mark_pitch(track, capacity);
            expect!((capacity - 1) as f64 * pitch <= track + 0.001);
        }
        track += 4.0;
    }
}

fn stack_compression_near_pointer(_mtm: MainThreadMarker) {
    let resting = DensityGutterView::centered_band_y_positions_default(800.0, 4);
    let compressed = DensityGutterView::centered_band_y_positions(
        800.0,
        4,
        28.0,
        DensityGutterView::MAX_PITCH,
        Some(resting[1]),
        DensityGutterView::STACK_COMPRESSION,
        DensityGutterView::PROXIMITY_RADIUS,
    );
    expect!(resting.len() == 4);
    expect!(compressed.len() == 4);
    let rest_gap = resting[2] - resting[1];
    let near_gap = compressed[2] - compressed[1];
    expect!(near_gap < rest_gap);
    expect!((compressed.first().unwrap() + compressed.last().unwrap()) / 2.0 == 400.0);
}

fn at_rest_bands(_mtm: MainThreadMarker) {
    let source = "# First\n\n- [ ] Ship rail\n\n```swift\nlet body = true\n```\n\n| Name |\n| --- |\n| body |\n\n## Second";
    let document = parse(source);
    let bands = DensityGutterView::bands_for(
        &document,
        &[(ChangeKind::Modified, NSRange::new(0, 5))],
        &[NSRange::new(10, 4)],
    );

    let has = |predicate: &dyn Fn(DensityBandKind) -> bool| bands.iter().any(|band| predicate(band.kind));
    expect!(has(&|kind| matches!(kind, DensityBandKind::Heading { .. })));
    expect!(has(&|kind| matches!(kind, DensityBandKind::TaskList)));
    expect!(has(&|kind| matches!(kind, DensityBandKind::SearchHit)));
    expect!(has(&|kind| matches!(kind, DensityBandKind::Change(_))));
    expect!(has(&|kind| matches!(kind, DensityBandKind::CodeBlock)));
    expect!(has(&|kind| matches!(kind, DensityBandKind::Table)));
    expect!(DensityGutterView::is_overlay(DensityBandKind::Change(ChangeKind::Modified)));
    expect!(DensityGutterView::is_overlay(DensityBandKind::SearchHit));
    expect!(!DensityGutterView::is_overlay(DensityBandKind::CodeBlock));
    expect!(!DensityGutterView::is_overlay(DensityBandKind::Table));
    expect!(!DensityGutterView::is_overlay(DensityBandKind::Math));
    expect!(!DensityGutterView::is_overlay(DensityBandKind::TaskList));
    expect!(!DensityGutterView::is_overlay(DensityBandKind::Image));
    expect!(!DensityGutterView::is_overlay(DensityBandKind::Callout));
}

fn heading(level: isize, fraction: f64) -> DensityBand {
    DensityBand::new(DensityBandKind::Heading { level }, fraction, fraction)
}

fn levels(bands: &[DensityBand]) -> Vec<isize> {
    bands
        .iter()
        .map(|band| match band.kind {
            DensityBandKind::Heading { level } => level,
            _ => -1,
        })
        .collect()
}

fn body_bands_are_not_indexed(_mtm: MainThreadMarker) {
    let bands = [
        heading(1, 0.0),
        DensityBand::new(DensityBandKind::CodeBlock, 0.1, 0.2),
        heading(1, 0.3),
        DensityBand::new(DensityBandKind::Table, 0.4, 0.5),
        DensityBand::new(DensityBandKind::Callout, 0.55, 0.6),
        heading(1, 0.7),
        DensityBand::new(DensityBandKind::Image, 0.8, 0.85),
    ];

    let selection = DensityGutterView::selection_for(&bands, 20, true);
    expect!(selection.marks.len() == 3);
    expect!(levels(&selection.marks) == vec![1, 1, 1]);
    expect!(selection.pips.iter().all(|pip| pip.is_empty()));
}

fn outline_entry_state(_mtm: MainThreadMarker) {
    let entry = DensityOutlineEntry::new("Section", 2, 0.4, true);
    expect!(entry == DensityOutlineEntry::new("Section", 2, 0.4, true));
    expect!(entry != DensityOutlineEntry::new("Section", 2, 0.4, false));
}

fn centered_band_positions(_mtm: MainThreadMarker) {
    let positions = DensityGutterView::centered_band_y_positions(
        800.0,
        4,
        28.0,
        20.0,
        None,
        DensityGutterView::STACK_COMPRESSION,
        DensityGutterView::PROXIMITY_RADIUS,
    );
    expect!(positions.len() == 4);
    expect!(positions == vec![370.0, 390.0, 410.0, 430.0]);
    expect!((positions.first().unwrap() + positions.last().unwrap()) / 2.0 == 400.0);
}

fn depth_budget_keeps_structure(_mtm: MainThreadMarker) {
    let mut bands = Vec::new();
    for index in 0..4 {
        bands.push(heading(1, f64::from(index) * 0.25));
        for child in 1..=4 {
            bands.push(heading(3, f64::from(index) * 0.25 + f64::from(child) * 0.04));
        }
    }

    let selected = DensityGutterView::select_headings(&bands, 6);
    expect!(selected.len() <= 6);
    expect!(levels(&selected).iter().filter(|level| **level == 1).count() == 4);
    let fractions: Vec<f64> = selected.iter().map(|band| band.start_fraction).collect();
    let mut sorted = fractions.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    expect!(sorted == fractions);
}

fn depth_budget_fills_spare_slots(_mtm: MainThreadMarker) {
    let mut bands = vec![heading(1, 0.0)];
    for index in 0..40 {
        bands.push(heading(2, f64::from(index + 1) / 41.0));
    }

    let selected = DensityGutterView::select_headings(&bands, 12);
    expect!(selected.len() == 12);
    expect!(levels(&selected).contains(&1));
    expect!(levels(&selected).iter().filter(|level| **level == 2).count() == 11);
}

fn depth_budget_keeps_shallow_documents(_mtm: MainThreadMarker) {
    let bands: Vec<DensityBand> = (0..5).map(|index| heading(index % 3 + 1, index as f64 / 5.0)).collect();
    expect!(DensityGutterView::select_headings(&bands, 20).len() == 5);
}

fn overlays_do_not_evict_headings(_mtm: MainThreadMarker) {
    let headings: Vec<DensityBand> = (0..8).map(|index| heading(1, f64::from(index) / 8.0)).collect();
    let hits: Vec<DensityBand> = (0..400)
        .map(|index| DensityBand::new(DensityBandKind::SearchHit, f64::from(index) / 400.0, f64::from(index) / 400.0))
        .collect();

    let clean = DensityGutterView::selection_for(&headings, 20, true);
    let mut all = headings.clone();
    all.extend(hits);
    let noisy = DensityGutterView::selection_for(&all, 20, true);

    expect!(clean.marks.len() == 8);
    expect!(noisy.marks.len() == 8);
    expect!(noisy.pips.iter().all(|pip| pip.search_hit));
}

fn overlay_pips_are_opt_in(mtm: MainThreadMarker) {
    let view = DensityGutterView::new_current(mtm);
    expect!(!view.shows_overlay_pips());

    let bands = [
        heading(1, 0.1),
        DensityBand::new(DensityBandKind::Change(ChangeKind::Modified), 0.15, 0.16),
        DensityBand::new(DensityBandKind::SearchHit, 0.17, 0.18),
    ];
    let selection = DensityGutterView::selection_for(&bands, 8, false);
    expect!(selection.marks.len() == 1);
    expect!(selection.pips.iter().all(|pip| pip.is_empty()));
}

fn pips_attach_to_enclosing_section(_mtm: MainThreadMarker) {
    let marks = [heading(1, 0.0), heading(1, 0.5), heading(1, 0.9)];
    let overlays = [
        DensityBand::new(DensityBandKind::Change(ChangeKind::Inserted), 0.52, 0.55),
        DensityBand::new(DensityBandKind::SearchHit, 0.95, 0.96),
        DensityBand::new(DensityBandKind::Change(ChangeKind::Deleted), 0.0, 0.0),
    ];

    let pips = DensityGutterView::pips(&overlays, &marks);
    expect!(pips[0].change == Some(ChangeKind::Deleted));
    expect!(pips[1].change == Some(ChangeKind::Inserted));
    expect!(!pips[1].search_hit);
    expect!(pips[2].search_hit);
    expect!(pips[2].change.is_none());
}

fn mixed_changes_collapse_to_modified(_mtm: MainThreadMarker) {
    let marks = [heading(1, 0.0), heading(1, 0.5)];
    let overlays = [
        DensityBand::new(DensityBandKind::Change(ChangeKind::Inserted), 0.1, 0.1),
        DensityBand::new(DensityBandKind::Change(ChangeKind::Deleted), 0.2, 0.2),
    ];
    expect!(DensityGutterView::pips(&overlays, &marks)[0].change == Some(ChangeKind::Modified));
}

fn sparse_documents_keep_heading_anchors(_mtm: MainThreadMarker) {
    let two: Vec<DensityBand> = (0..2).map(|index| heading(1, f64::from(index) / 2.0)).collect();
    expect!(DensityGutterView::selection_for(&two, 20, true).marks.len() == 2);

    let three: Vec<DensityBand> = (0..3).map(|index| heading(1, f64::from(index) / 3.0)).collect();
    expect!(DensityGutterView::selection_for(&three, 20, true).marks.len() == 3);

    let many: Vec<DensityBand> = (0..40).map(|index| heading(1, f64::from(index) / 40.0)).collect();
    let tiny_capacity = DensityGutterView::stack_capacity(20.0);
    expect!(DensityGutterView::selection_for(&many, tiny_capacity, true).marks.is_empty());
}

fn headingless_documents_index_overlays(_mtm: MainThreadMarker) {
    let changes: Vec<DensityBand> = (0..6)
        .map(|index| {
            DensityBand::new(DensityBandKind::Change(ChangeKind::Modified), f64::from(index) / 6.0, f64::from(index) / 6.0)
        })
        .collect();
    let selection = DensityGutterView::selection_for(&changes, 20, true);
    expect!(selection.marks.len() == 6);
    expect!(selection.pips.iter().all(|pip| pip.is_empty()));
}

fn stride_sampling_keeps_ends(_mtm: MainThreadMarker) {
    let bands: Vec<DensityBand> = (0..40).map(|index| heading(1, f64::from(index) / 40.0)).collect();
    let sampled = DensityGutterView::stride_sampled(&bands, 9);
    expect!(sampled.len() == 9);
    expect!(sampled.first().unwrap().start_fraction == bands.first().unwrap().start_fraction);
    expect!(sampled.last().unwrap().start_fraction == bands.last().unwrap().start_fraction);
    expect!(DensityGutterView::stride_sampled(&bands, 1).len() == 1);
    expect!(DensityGutterView::stride_sampled(&bands, 0).is_empty());
    expect!(DensityGutterView::stride_sampled(&bands, 100).len() == 40);
}

fn current_heading_policy(_mtm: MainThreadMarker) {
    let bands = [
        DensityBand::new(DensityBandKind::Heading { level: 1 }, 0.1, 0.1),
        DensityBand::new(DensityBandKind::Heading { level: 2 }, 0.3, 0.3),
        DensityBand::new(DensityBandKind::Heading { level: 2 }, 0.8, 0.8),
    ];
    expect!(DensityGutterView::current_heading_fraction_in(&bands, (0.35, 0.6)) == Some(0.3));
    expect!(DensityGutterView::current_heading_fraction_in(&bands, (0.0, 0.05)) == Some(0.1));
}

fn scrub_activation_has_a_real_threshold(_mtm: MainThreadMarker) {
    let origin = NSPoint::new(20.0, 100.0);
    expect!(!DensityGutterView::should_begin_scrub(origin, NSPoint::new(21.0, 102.0)));
    expect!(DensityGutterView::should_begin_scrub(origin, NSPoint::new(20.0, 104.0)));
}

fn detent_fires_on_arrival_and_crossing_only(_mtm: MainThreadMarker) {
    expect!(DensityGutterView::is_detent_crossing(None, Some(0)));
    expect!(DensityGutterView::is_detent_crossing(Some(0), Some(1)));
    expect!(!DensityGutterView::is_detent_crossing(Some(1), Some(1)));
    expect!(!DensityGutterView::is_detent_crossing(Some(0), None));
    expect!(!DensityGutterView::is_detent_crossing(None, None));
}

/// Pinned rather than inherited: a runner with Reduce Motion set silences
/// the rail, and a tap-counting test would then pass by measuring nothing.
fn rail(reduce_motion: bool, mtm: MainThreadMarker) -> objc2::rc::Retained<DensityGutterView> {
    let app_appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    let rail = DensityGutterView::new(
        Rc::new(StyleSheet::new(ThemeStore::shared().current(), &app_appearance, Some(reduce_motion))),
        mtm,
    );
    rail.setFrame(rect(0.0, 0.0, DensityGutterView::WIDTH, 600.0));
    rail.set_bands(
        (0..40)
            .map(|index| {
                DensityBand::new(
                    DensityBandKind::Heading { level: 1 },
                    f64::from(index) / 40.0,
                    f64::from(index + 1) / 40.0,
                )
            })
            .collect(),
    );
    rail.layoutSubtreeIfNeeded();
    rail
}

fn detents_are_rate_limited_across_a_scrub(mtm: MainThreadMarker) {
    let rail = rail(false, mtm);
    let taps = Rc::new(Cell::new(0usize));
    let counter = taps.clone();
    rail.set_perform_haptic_feedback(Rc::new(move || counter.set(counter.get() + 1)));

    let mut crossings = 0usize;
    let mut previous: Option<isize> = None;
    let positions = rail.mark_positions_for_testing();
    for step in 0..=600 {
        let y = f64::from(step);
        let next_index = DensityGutterView::next_hovered_band_index(
            y,
            &positions,
            previous,
            DensityGutterView::HOVER_ACTIVATION_SLOP,
            DensityGutterView::dismissal_slop(&positions),
        );
        if DensityGutterView::is_detent_crossing(previous, next_index) {
            crossings += 1;
        }
        previous = next_index;
        rail.drive_hover_for_testing(y);
    }

    expect!(crossings > 20, "the sweep did not actually cross the stack");
    expect!(taps.get() > 0, "a sweep across the stack produced no detent at all");
    expect!(
        taps.get() < crossings / 4,
        "{} taps for {crossings} crossings — the rail is buzzing, not ticking",
        taps.get()
    );

    let before = taps.get();
    std::thread::sleep(std::time::Duration::from_secs_f64(DensityGutterView::DETENT_INTERVAL * 2.0));
    rail.drive_hover_for_testing(*positions.first().expect("a drawn mark"));
    expect!(taps.get() > before, "the rail stopped tapping after its first detent");
}

fn reduce_motion_silences_the_rail(mtm: MainThreadMarker) {
    let quiet = rail(true, mtm);
    let taps = Rc::new(Cell::new(0usize));
    let counter = taps.clone();
    quiet.set_perform_haptic_feedback(Rc::new(move || counter.set(counter.get() + 1)));
    let positions = quiet.mark_positions_for_testing();

    for step in 0..=600 {
        quiet.drive_hover_for_testing(f64::from(step));
    }
    expect!(taps.get() == 0, "the rail tapped {} times with Reduce Motion set", taps.get());

    let loud = rail(false, mtm);
    let loud_taps = Rc::new(Cell::new(0usize));
    let counter = loud_taps.clone();
    loud.set_perform_haptic_feedback(Rc::new(move || counter.set(counter.get() + 1)));
    expect!(loud.mark_positions_for_testing() == positions);
    for step in 0..=600 {
        loud.drive_hover_for_testing(f64::from(step));
    }
    expect!(loud_taps.get() > 0);
}

#[allow(dead_code)]
fn _unused(_: &NSString) {}
