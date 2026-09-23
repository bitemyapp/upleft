//! Port of the `Tests/DownrightAppTests/WindowChromeTests.swift` cases that
//! exercise `DocumentWindowController`'s toolbar and command dispatch
//! (`+Actions`, `+Commands`, `+Delegates`): the toolbar's items, the Find
//! button's toggle, `perform(_:)` in split view, Use Selection for Find, and
//! the find bar's delegate path. `window_chrome_tests.rs` holds the
//! controller-free cases.
//!
//! The rest of the Swift suite's controller cases (`missingFileRecovery…`,
//! `splitViewUsesTwoVisibleSideBySideDocumentPanes`,
//! `splitDividerIsThemedChrome…`, `documentBarsReserveSpace…`,
//! `floatingTaskPanelFitsItsFooterRow`, `inspectorSelectionAndClose…`,
//! `replaceModePreservesActiveQuery`, `localFindPreservesViewport`,
//! `findMotionDoesNotShiftDocument`, `ordinaryFindDoesNotReplace…`,
//! `closingTheFindBarRetires…`, `statusBarIsOffByDefault…`) test the main
//! controller file's panels and layout, not these extensions; they belong
//! with its port.
//!
//! The Swift suite is `@MainActor` and `.serialized`: this binary owns the
//! main thread (`harness = false`). No window is ordered in
//! (`controller_support`); Swift's `DocumentWindowController()` never orders
//! its window in either, so nothing is adapted.

mod controller_support;

use controller_support::{Closing, Removing, descendants, new_controller, temporary_directory};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAccessibilityProtocol, NSButton, NSLayoutAttribute, NSTextField, NSTitlebarSeparatorStyle,
    NSToolbarDisplayMode, NSToolbarFlexibleSpaceItemIdentifier, NSView, NSWindowStyleMask, NSWindowTitleVisibility,
    NSWindowToolbarStyle,
};
use objc2_foundation::NSString;
use upleft_app::app::main_menu::MainMenu;
use upleft_app::app::toolbar_controls::{
    ToolbarActionButton, ToolbarDocumentIdentityView, ToolbarMenuButton, ToolbarPresentationControl,
    ToolbarTrailingCluster,
};
use upleft_app::panels::activity_indicator_view::ActivityIndicatorView;
use upleft_app::panels::appkit_support::{downcast, is};
use upleft_app::panels::task_progress_ring::TaskProgressRing;
use upleft_app::panels::update_status_pill::UpdateStatusPill;
use upleft_app::support::commands::Command;
use upleft_core::{NSRange, ZoomLevel};
use upleft_render::render_contracts::RenderMode;

fn same_object(a: &AnyObject, b: &AnyObject) -> bool {
    std::ptr::eq(a, b)
}

fn accessibility_label(view: &NSView) -> Option<String> {
    view.accessibilityLabel().map(|label| label.to_string())
}

fn find_button(label: &str, root: &NSView) -> Retained<NSButton> {
    descendants(root)
        .into_iter()
        .filter_map(|view| downcast::<NSButton>(&view))
        .find(|button| accessibility_label(button).as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no button labelled {label}"))
}

fn find_text_field(label: &str, root: &NSView) -> Retained<NSTextField> {
    descendants(root)
        .into_iter()
        .filter_map(|view| downcast::<NSTextField>(&view))
        .find(|field| accessibility_label(field).as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no text field labelled {label}"))
}

fn toolbar_uses_native_centered_mode_and_trailing_menu() {
    let controller = Closing(new_controller());
    let window = controller.window().expect("the document window");
    let toolbar = window.toolbar().expect("the toolbar");

    assert!(!window.isRestorable());
    let delegate = window.delegate().expect("the window delegate");
    assert!(same_object(delegate.as_ref(), &controller.0));
    assert!(window.titlebarAppearsTransparent());
    assert!(window.styleMask().contains(NSWindowStyleMask::FullSizeContentView));
    assert_eq!(window.titlebarSeparatorStyle(), NSTitlebarSeparatorStyle::None);
    assert!(controller.toolbar_glass_band().is_some());
    assert_eq!(window.toolbarStyle(), NSWindowToolbarStyle::Unified);
    let gutter: &AnyObject = controller.density_gutter_view();
    assert!(controller.primary_container().leading_accessory().is_some_and(|view| same_object(&view, gutter)));
    assert!(controller.primary_container().trailing_accessory().is_none());
    assert_eq!(toolbar.displayMode(), NSToolbarDisplayMode::IconOnly);
    assert_eq!(toolbar.identifier().to_string(), "DownrightToolbar.v11");
    assert_eq!(toolbar.centeredItemIdentifier().map(|identifier| identifier.to_string()).as_deref(), Some("presentation-mode"));
    // SAFETY: AppKit's immutable identifier constant.
    let flexible_space = unsafe { NSToolbarFlexibleSpaceItemIdentifier }.to_string();
    let identifiers: Vec<String> =
        controller.toolbar_default_item_identifiers(&toolbar).iter().map(|identifier| identifier.to_string()).collect();
    assert_eq!(
        identifiers,
        vec![
            "document-identity".to_owned(),
            flexible_space.clone(),
            "presentation-mode".to_owned(),
            flexible_space,
            "trailing-cluster".to_owned(),
        ]
    );

    let items = toolbar.items();
    let item_view = |identifier: &str| {
        items.iter().find(|item| item.itemIdentifier().to_string() == identifier).and_then(|item| item.view())
    };
    let identity = item_view("document-identity")
        .and_then(|view| downcast::<ToolbarDocumentIdentityView>(&view))
        .expect("the identity view");
    assert_eq!(identity.intrinsicContentSize().width, 220.0);
    assert_eq!(identity.intrinsicContentSize().height, 36.0);
    assert_eq!(window.titleVisibility(), NSWindowTitleVisibility::Hidden);

    let mode = item_view("presentation-mode")
        .and_then(|view| downcast::<ToolbarPresentationControl>(&view))
        .expect("the presentation control");
    assert!(!mode.isHidden());
    assert_eq!(mode.segment_titles(), vec!["Document".to_owned(), "Source".to_owned()]);
    assert_eq!(mode.selected_segment(), 0);
    assert_eq!(mode.intrinsicContentSize().width, 184.0);
    assert_eq!(mode.intrinsicContentSize().height, 34.0);

    controller.primary_container().text_view().focus_entire_source();
    controller.refresh_source_focus_toolbar();
    assert_eq!(mode.selected_segment(), 1);
    controller.primary_container().text_view().clear_source_focus();
    controller.refresh_source_focus_toolbar();
    assert_eq!(mode.selected_segment(), 0);

    // The trailing controls ship as one cluster item rather than five
    // separate ones: AppKit pads every custom-view item by its own margin —
    // a tax even a hidden placeholder pays — which scattered the row with
    // uneven gaps and stretched the button plates to 36pt beside the ring's
    // 30pt one. The stack owns the spacing now, so the row reads as one
    // tight unit against the trailing edge.
    let cluster = item_view("trailing-cluster")
        .and_then(|view| downcast::<ToolbarTrailingCluster>(&view))
        .expect("the trailing cluster is not a toolbar item");
    assert_eq!(cluster.spacing(), 8.0);
    assert_eq!(cluster.alignment(), NSLayoutAttribute::CenterY);
    let arranged: Vec<Retained<NSView>> = cluster.arrangedSubviews().to_vec();
    // Find is in the cluster, not inside the `···` overflow. The overflow
    // used to be the only interactive control on the trailing edge, which
    // put every panel in the app behind one unlabelled glyph and a menu —
    // in a window with room for more buttons.
    let find = arranged
        .iter()
        .filter_map(|view| downcast::<ToolbarActionButton>(view))
        .find(|button| accessibility_label(button).as_deref() == Some("Find"))
        .expect("find is not in the trailing cluster");
    assert_eq!(find.intrinsicContentSize().width, 34.0);
    assert_eq!(find.intrinsicContentSize().height, 34.0);
    assert!(find.uses_glass_surface_for_testing());
    assert_eq!(find.feedback_inset_x(), 0.0);
    assert_eq!(find.feedback_inset_y(), 0.0);
    assert_eq!(find.feedback_corner_radius(), 17.0);
    assert!(!find.is_on(), "find should rest unlit with its panel closed");
    assert!(
        !arranged.iter().filter_map(|view| downcast::<ToolbarActionButton>(view)).any(|button| {
            accessibility_label(&button).as_deref() == Some(Command::DocumentLens.title())
        }),
        "Contents / Outline belongs in menus, not the permanent toolbar"
    );
    assert!(arranged.iter().any(|view| is::<ActivityIndicatorView>(view)));
    assert!(arranged.iter().any(|view| is::<TaskProgressRing>(view)));
    let update_pill = arranged
        .iter()
        .find_map(|view| downcast::<UpdateStatusPill>(view))
        .expect("update status pill is not in the trailing cluster");
    assert!(update_pill.title().to_string().is_empty(), "the custom pill must not draw NSButton's title");
    assert!(!items.iter().any(|item| item.itemIdentifier().to_string() == "contents"));
    assert!(!items.iter().any(|item| item.itemIdentifier().to_string() == "inspector"));
    let overflow = arranged
        .iter()
        .find_map(|view| downcast::<ToolbarMenuButton>(view))
        .expect("the overflow menu is not in the trailing cluster");
    let popup_items = overflow.popup_menu_items();
    assert!(!popup_items.iter().any(|item| MainMenu::command(item) == Some(Command::DocumentLens)));
    assert_eq!(overflow.intrinsicContentSize().width, 34.0);
    assert_eq!(overflow.intrinsicContentSize().height, 34.0);
    assert!(popup_items.iter().any(|item| item.title().to_string() == "Document Detail"));
    assert!(popup_items.iter().any(|item| {
        let title = item.title().to_string();
        title == "Source Focus" || title == "Exit Source Focus"
    }));
    // The cluster leads with the spinner and ends at the menu: activity,
    // find, ring, pill, overflow — each hidden view costs nothing.
    assert!(arranged.first().is_some_and(|view| is::<ActivityIndicatorView>(view)));
    assert!(arranged.last().is_some_and(|view| is::<ToolbarMenuButton>(view)));
}

fn split_view_mirrors_presentation_state() {
    let controller = Closing(new_controller());

    controller.toggle_split_view();
    assert!(controller.perform(Command::SourceMode));
    assert_eq!(controller.primary_container().text_view().mode(), RenderMode::Source);
    assert_eq!(controller.split_container().map(|split| split.text_view().mode()), Some(RenderMode::Source));

    assert!(controller.perform(Command::ZoomLevel1));
    assert_eq!(controller.primary_container().text_view().zoom_level(), ZoomLevel::H1);
    assert_eq!(controller.split_container().map(|split| split.text_view().zoom_level()), Some(ZoomLevel::H1));
}

fn local_find_uses_compact_document_bar() {
    use upleft_app::panels::find_bar_view::FindBarDensity;

    let controller = Closing(new_controller());

    controller.show_find_bar(false, None);

    let bar = controller.find_bar().expect("the find bar");
    let bar_stack = controller.bar_stack();
    assert!(bar.superview().is_some());
    assert!(!bar.superview().is_some_and(|superview| same_object(&superview, &bar_stack)));
    assert_eq!(bar.intrinsicContentSize().height, FindBarDensity::BAR_HEIGHT);
    assert_eq!(bar.divider_count_for_testing(), 2);
    assert!(bar.has_close_button_for_testing());
    assert!(!bar.search_field_is_bezeled_for_testing());

    let find_button = controller.toolbar_find_button().expect("the toolbar's Find button");
    find_button.performClick(None);
    assert!(controller.find_bar().is_none());
    find_button.performClick(None);
    assert!(controller.find_bar().is_some());

    controller.dismiss_find_bar();
    assert!(controller.find_bar().is_none());
}

fn selection_find_ignores_an_empty_selection() {
    let root = temporary_directory("upleft-find-empty-selection");
    let _remove = Removing(root.clone());
    let file = root.appending_path_component("note.md");
    std::fs::write(file.path(), "alpha beta alpha\n").unwrap();

    let controller = Closing(new_controller());
    controller.open(&file, RenderMode::Live).unwrap();
    controller.primary_container().text_view().set_source_selected_ranges(&[NSRange::new(0, 0)]);

    let _ = controller.perform(Command::UseSelectionForFind);

    assert!(controller.find_bar().is_none());
}

fn find_action_flushes_the_visible_query_before_the_debounce_fires() {
    let root = temporary_directory("upleft-find-action");
    let _remove = Removing(root.clone());
    let file = root.appending_path_component("note.md");
    std::fs::write(file.path(), "alpha beta alpha\n").unwrap();

    let controller = Closing(new_controller());
    controller.open(&file, RenderMode::Live).unwrap();
    controller.show_find_bar(false, None);
    let bar = controller.find_bar().expect("the find bar");

    bar.set_query_text("alpha", true);
    find_button("Next match", &bar).performClick(None);

    assert_eq!(controller.current_find_query().text, "alpha");
    assert_eq!(bar.status_text(), "2 of 2");

    bar.set_shows_replace(true);
    let replacement = find_text_field("Replace with", &bar);
    bar.set_query_text("beta", true);
    replacement.setStringValue(&NSString::from_str("gamma"));
    find_button("Replace", &bar).performClick(None);
    assert_eq!(controller.markdown_document().text(), "alpha gamma alpha\n");

    bar.set_query_text("alpha", true);
    replacement.setStringValue(&NSString::from_str("omega"));
    find_button("All", &bar).performClick(None);
    assert_eq!(controller.markdown_document().text(), "omega gamma omega\n");
}

fn main() {
    controller_support::prepare();
    controller_support::main_thread::run(&[
        ("toolbar_uses_native_centered_mode_and_trailing_menu", toolbar_uses_native_centered_mode_and_trailing_menu),
        ("split_view_mirrors_presentation_state", split_view_mirrors_presentation_state),
        ("local_find_uses_compact_document_bar", local_find_uses_compact_document_bar),
        ("selection_find_ignores_an_empty_selection", selection_find_ignores_an_empty_selection),
        (
            "find_action_flushes_the_visible_query_before_the_debounce_fires",
            find_action_flushes_the_visible_query_before_the_debounce_fires,
        ),
    ]);
    controller_support::finish();
}
