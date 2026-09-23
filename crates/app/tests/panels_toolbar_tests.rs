//! View-level tests for BreadcrumbView, DocumentStatusBarView, TaskProgressRing
//! and CommandPaletteView (ported from DownrightAppTests). Runs on the main
//! thread; no test puts a window on screen.
//!
//! Ported:
//! - `WindowChromeTests`: `breadcrumbReservesAStableTextSafeLane`,
//!   `breadcrumbAppearsOnlyWhenPresented`,
//!   `breadcrumbShowsOnlyTheCurrentSection`,
//!   `breadcrumbPathComparisonAvoidsScrollTimeRebuilds`.
//!
//! - `DocumentChromeLayoutTests.TaskProgressRingAccessibilityTests` (all
//!   six).
//! - `WindowChromeTests.statusBarIsOffByDefaultAndCostsNoHeightWhenHidden`,
//!   adapted: the preference default and the bar's own `isVisible` contract
//!   on a bar built directly (the Swift test reads the bar through
//!   `DocumentWindowController`).
//!
//! Skipped (they need `DocumentWindowController`, which is App/ and not
//! ported): `WindowChromeTests.toolbarUsesNativeCenteredModeAndTrailingMenu`
//! (the ring in the trailing cluster),
//! `CommandPaletteNavigationRegressionTests` (both tests), and the
//! `DocumentChromeLayoutTests` cases that drive `controller.progressRing`.

#[path = "main_thread/mod.rs"]
mod main_thread;

use objc2::{AnyThread, MainThreadMarker};
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSControlStateValueOff, NSControlStateValueOn, NSTextStorage, NSView};
use objc2_foundation::{NSSize, NSString};
use upleft_app::panels::appkit_support::{RectExt, accessibility_label, downcast};
use upleft_app::panels::breadcrumb_view::{BreadcrumbView, Crumb};
use upleft_app::panels::document_status_bar_view::DocumentStatusBarView;
use upleft_app::panels::task_progress_ring::TaskProgressRing;
use upleft_app::support::preferences::Values as PreferenceValues;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn container() -> Retained<MarkdownContainerView> {
    let storage: Retained<NSTextStorage> =
        unsafe { objc2::msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str("Hello")] };
    MarkdownContainerView::with_storage(&storage, mtm())
}

fn layout(view: &NSView) {
    let _: () = unsafe { objc2::msg_send![view, layout] };
}

// MARK: - WindowChromeTests (breadcrumb)

fn breadcrumb_reserves_a_stable_text_safe_lane() {
    let container = container();
    let crumb = BreadcrumbView::new_current(mtm());
    crumb.set_trail(vec![Crumb::new(0, "Root", 1), Crumb::new(1, "Section", 2)]);
    container.set_top_accessory(Some(Retained::into_super(crumb.clone())));
    container.set_top_accessory_overlays_content(false);
    container.setFrameSize(NSSize::new(900.0, 600.0));
    layout(&container);
    assert!(container.scroll_view().frame().min_y() > 0.0);
    assert!(crumb.frame().max_y() <= container.scroll_view().frame().min_y());
}

fn breadcrumb_appears_only_when_presented() {
    let container = container();
    let crumb = BreadcrumbView::new_current(mtm());
    container.set_top_accessory(Some(Retained::into_super(crumb.clone())));
    container.set_top_accessory_overlays_content(false);
    container.setFrameSize(NSSize::new(900.0, 600.0));
    layout(&container);
    assert!(!crumb.is_presented_for_testing());

    crumb.set_trail(vec![Crumb::new(0, "Section", 1)]);
    crumb.show_current_section();
    layout(&container);
    assert!(crumb.is_presented_for_testing());
    let document_origin = container.scroll_view().frame().min_y();
    assert!(document_origin > 0.0);

    crumb.hide_current_section();
    assert!(!crumb.is_presented_for_testing());
    layout(&container);
    assert_eq!(container.scroll_view().frame().min_y(), document_origin);
}

fn breadcrumb_shows_only_the_current_section() {
    let crumb = BreadcrumbView::new_current(mtm());
    crumb.set_trail(vec![Crumb::new(0, "Downright Design", 1), Crumb::new(1, "Typography and colour", 2)]);
    crumb.setFrameSize(NSSize::new(720.0, 28.0));
    layout(&crumb);

    let button = crumb
        .subviews()
        .iter()
        .find_map(|view| downcast::<NSButton>(&view))
        .expect("the section button");
    assert_eq!(button.attributedTitle().string().to_string(), "Typography and colour");
    assert_eq!(accessibility_label(&*button).as_deref(), Some("Current section: Typography and colour"));
    assert!(!button.isHidden());
    assert_eq!(crumb.current_title_origin(), 0.0);

    let menu = crumb.make_path_menu();
    let items = menu.itemArray();
    let titles: Vec<String> = items.iter().map(|item| item.title().to_string()).collect();
    assert_eq!(titles, ["Downright Design", "Typography and colour"]);
    let indentation: Vec<isize> = items.iter().map(|item| item.indentationLevel()).collect();
    assert_eq!(indentation, [0, 1]);
    let states: Vec<isize> = items.iter().map(|item| item.state()).collect();
    assert_eq!(states, [NSControlStateValueOff, NSControlStateValueOn]);
}

fn breadcrumb_path_comparison_avoids_scroll_time_rebuilds() {
    let path = [Crumb::new(0, "Root", 1), Crumb::new(4, "Section", 2)];
    assert!(BreadcrumbView::same_trail(&path, &path));
    assert!(!BreadcrumbView::same_trail(&path, &[Crumb::new(0, "Root", 1)]));
    assert!(!BreadcrumbView::same_trail(&path, &[Crumb::new(0, "Root", 1), Crumb::new(5, "Next", 2)]));
}

// MARK: - WindowChromeTests (status bar)

/// DESIGN.md's "Avoid" list names a permanent status bar outright, so the
/// bar ships hidden and costs no height until View ▸ Status Bar asks for it.
fn status_bar_is_off_by_default_and_costs_no_height_when_hidden() {
    assert!(!PreferenceValues::default().show_status_bar);
    let bar = DocumentStatusBarView::new(std::rc::Rc::new(StyleSheet::current(mtm())), mtm());

    bar.set_is_visible(false);
    assert!(bar.isHidden());
    assert_eq!(bar.intrinsicContentSize().height, 0.0);

    bar.set_is_visible(true);
    assert!(!bar.isHidden());
    assert!(bar.intrinsicContentSize().height > 0.0);
}

// MARK: - TaskProgressRingAccessibilityTests

fn ring() -> Retained<TaskProgressRing> {
    TaskProgressRing::new_current(mtm())
}

fn tool_tip(view: &NSView) -> Option<String> {
    view.toolTip().map(|tip| tip.to_string())
}

/// An empty plan still names itself.
fn empty_ring_is_labelled() {
    let ring = ring();
    ring.set_progress(0, 0);
    assert_eq!(accessibility_label(&*ring).as_deref(), Some("No tasks"));
    assert_eq!(tool_tip(&ring).as_deref(), Some("No tasks — Open Tasks"));
    let role: Option<Retained<NSString>> = unsafe { objc2::msg_send![&*ring, accessibilityRole] };
    assert_eq!(role.map(|role| role.to_string()).as_deref(), Some("AXButton"));
    let can_move: bool = unsafe { objc2::msg_send![&*ring, mouseDownCanMoveWindow] };
    assert!(!can_move);
}

/// A partly finished plan reports the count and the remainder.
fn partial_ring_reports_remainder() {
    let ring = ring();
    ring.set_progress(3, 7);
    assert_eq!(ring.count_text_for_testing(), "4");
    assert_eq!(accessibility_label(&*ring).as_deref(), Some("3 of 7 tasks complete"));
    assert_eq!(tool_tip(&ring).as_deref(), Some("3 of 7 tasks complete, 4 left — Open Tasks"));
}

/// One remaining task is not pluralised.
fn single_remainder_reads_naturally() {
    let ring = ring();
    ring.set_progress(6, 7);
    assert_eq!(ring.count_text_for_testing(), "1");
    assert!(tool_tip(&ring).is_some_and(|tip| tip.contains("1 left")));
}

/// An open panel gives the tally to the panel.
fn active_ring_hides_drawn_count() {
    let ring = ring();
    ring.set_progress(3, 7);
    ring.set_is_active(true);
    assert!(ring.count_text_for_testing().is_empty());
}

/// A finished plan says so rather than saying nothing.
fn complete_ring_reports_completion() {
    let ring = ring();
    ring.set_progress(5, 5);
    assert_eq!(accessibility_label(&*ring).as_deref(), Some("5 of 5 tasks complete"));
    assert!(tool_tip(&ring).is_some_and(|tip| tip.contains("all done")));
}

/// A long plan truncates the drawn numeral to "99+", so the exact figure
/// has to survive somewhere.
fn long_plan_keeps_exact_figure() {
    let ring = ring();
    ring.set_progress(5, 400);
    assert_eq!(accessibility_label(&*ring).as_deref(), Some("5 of 400 tasks complete"));
    assert!(tool_tip(&ring).is_some_and(|tip| tip.contains("395 left")));
}

fn main() {
    main_thread::run(&[
        ("breadcrumb_reserves_a_stable_text_safe_lane", breadcrumb_reserves_a_stable_text_safe_lane),
        ("breadcrumb_appears_only_when_presented", breadcrumb_appears_only_when_presented),
        ("breadcrumb_shows_only_the_current_section", breadcrumb_shows_only_the_current_section),
        ("breadcrumb_path_comparison_avoids_scroll_time_rebuilds", breadcrumb_path_comparison_avoids_scroll_time_rebuilds),
        ("empty_ring_is_labelled", empty_ring_is_labelled),
        ("partial_ring_reports_remainder", partial_ring_reports_remainder),
        ("single_remainder_reads_naturally", single_remainder_reads_naturally),
        ("active_ring_hides_drawn_count", active_ring_hides_drawn_count),
        ("complete_ring_reports_completion", complete_ring_reports_completion),
        ("long_plan_keeps_exact_figure", long_plan_keeps_exact_figure),
        (
            "status_bar_is_off_by_default_and_costs_no_height_when_hidden",
            status_bar_is_off_by_default_and_costs_no_height_when_hidden,
        ),
    ]);
}
