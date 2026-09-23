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
//! Skipped (they need `DocumentWindowController`, which is App/ and not
//! ported): `WindowChromeTests.toolbarUsesNativeCenteredModeAndTrailingMenu`
//! (the ring in the trailing cluster),
//! `WindowChromeTests.statusBarIsOffByDefaultAndCostsNoHeightWhenHidden`,
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

fn main() {
    main_thread::run(&[
        ("breadcrumb_reserves_a_stable_text_safe_lane", breadcrumb_reserves_a_stable_text_safe_lane),
        ("breadcrumb_appears_only_when_presented", breadcrumb_appears_only_when_presented),
        ("breadcrumb_shows_only_the_current_section", breadcrumb_shows_only_the_current_section),
        ("breadcrumb_path_comparison_avoids_scroll_time_rebuilds", breadcrumb_path_comparison_avoids_scroll_time_rebuilds),
    ]);
}
