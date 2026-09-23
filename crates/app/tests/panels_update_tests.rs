//! View-level tests for UpdateWindowController, UpdateNotesPopover and
//! UpdateStatusPill (ported from DownrightAppTests). Runs on the main
//! thread; no test builds a window.
//!
//! Ported (each Swift `@Test` keeps its name, as `Suite/test`, and its
//! assertions; fixtures are rebranded as in `update_coordinator_tests`):
//!
//! - `UpdateCoordinatorTests.swift`: the view halves of
//!   `UpdatePanelFooterTests/rebuildsAfterCheckIntoAvailableState` and
//!   `UpdatePanelTransitionTests/rendersEveryUpdaterPhaseWithoutThrowing`
//!   (their coordinator halves run in `update_coordinator_tests`), and all of
//!   `UpdateNotesSummaryTests` and `UpdateReleaseNotesReductionTests`.
//! - `WindowChromeTests.swift`: the pill's one assertion in
//!   `documentToolbarKeepsTrailingControlsInOneCluster` (the custom pill
//!   must not draw `NSButton`'s title), on a standalone pill.
//!
//! Skipped (listed when the binary runs): the rest of that
//! `WindowChromeTests` test, which needs `DocumentWindowController`'s
//! toolbar.

#[path = "main_thread/mod.rs"]
mod main_thread;

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSApplication};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString};
use upleft_app::panels::update_notes_popover::UpdateNotesSummary;
use upleft_app::panels::update_status_pill::UpdateStatusPill;
use upleft_app::panels::update_window_controller::{UpdateNotesView, UpdatePanelFooter, UpdatePanelView};
use upleft_app::updater::downright_update_driver::UpdateDriverHost;
use upleft_app::updater::update_coordinator::UpdateCoordinator;
use upleft_app::updater::update_engine::{FakeUpdateEngine, UpdateEngine};
use upleft_app::updater::update_metadata::{UpdateMetadata, Url};
use upleft_app::updater::update_state_machine::{UpdatePhase, UpdateStage};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

// MARK: - Fixtures

fn sample_metadata() -> UpdateMetadata {
    UpdateMetadata {
        version_string: "47".into(),
        display_version_string: "1.1.0".into(),
        title: Some("Upleft 1.1.0".into()),
        item_description: Some("## What's new\n- Faster parsing".into()),
        release_notes_url: None,
        info_url: Url::from_string("https://github.com/bitemyapp/upleft/releases/tag/v1.1.0"),
        content_length: 4_000_000,
        is_information_only: false,
        is_major_upgrade: false,
        is_critical: false,
        minimum_system_version: None,
    }
}

fn informational_metadata() -> UpdateMetadata {
    UpdateMetadata {
        version_string: "50".into(),
        display_version_string: "1.2.0".into(),
        title: None,
        item_description: None,
        release_notes_url: None,
        info_url: Url::from_string("https://github.com/bitemyapp/upleft/releases/tag/v1.2.0"),
        content_length: 0,
        is_information_only: true,
        is_major_upgrade: false,
        is_critical: false,
        minimum_system_version: None,
    }
}

/// `_ = NSApplication.shared; UpdateCoordinator(engine:)` with UI suppressed
/// and the engine started, as the Swift suites build it: nothing here may
/// open the update window.
fn make_coordinator() -> (Rc<UpdateCoordinator>, Rc<FakeUpdateEngine>) {
    let _ = NSApplication::sharedApplication(mtm());
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    let _ = engine.start();
    (coordinator, engine)
}

fn ns_error(domain: &str, code: isize) -> Retained<NSError> {
    unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(domain), code, None) }
}

// MARK: - UpdatePanelFooterTests

/// "Sparkle first presents the checking state, then replaces its Cancel
/// button with the result actions. The footer has two columns, so this
/// transition must remove each button from its owning stack only."
fn rebuilds_after_check_into_available_state() {
    let (coordinator, _engine) = make_coordinator();
    let footer = UpdatePanelFooter::with_frame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(540.0, 34.0)), mtm());
    coordinator.driver_did_begin_user_check(Box::new(|| {}));
    footer.update(&coordinator);

    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, Box::new(|_| {}));
    footer.update(&coordinator);

    assert_eq!(coordinator.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));
}

// MARK: - UpdatePanelTransitionTests

/// "Every Sparkle callback can rebuild the panel. Keep this smoke test on
/// the real AppKit view tree so a future state-specific layout regression
/// fails in CI before a release can ship it."
fn renders_every_updater_phase_without_throwing() {
    let (coordinator, _engine) = make_coordinator();

    let panel = UpdatePanelView::new(&coordinator, mtm());
    let render = || panel.refresh();

    coordinator.driver_did_begin_user_check(Box::new(|| {}));
    render();

    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, Box::new(|_| {}));
    render();

    coordinator.driver_did_begin_download(Box::new(|| {}));
    coordinator.driver_did_receive_expected_length(1_000);
    coordinator.driver_did_receive_data(250);
    render();

    coordinator.driver_did_begin_extraction();
    coordinator.driver_did_receive_extraction_progress(0.5);
    render();

    coordinator.driver_did_become_ready_to_relaunch(Box::new(|_| {}));
    render();

    coordinator.driver_did_begin_installation(false, Box::new(|| {}));
    render();

    coordinator.driver_did_begin_installation(true, Box::new(|| {}));
    render();

    coordinator.driver_did_begin_user_check(Box::new(|| {}));
    coordinator.driver_did_find_no_update(true, Box::new(|| {}));
    render();

    coordinator.driver_did_begin_user_check(Box::new(|| {}));
    coordinator.driver_did_encounter_error(&ns_error("sparkle", 2001), Box::new(|| {}));
    render();

    coordinator.driver_did_find_update(informational_metadata(), UpdateStage::NotDownloaded, true, Box::new(|_| {}));
    render();
}

// MARK: - UpdateNotesSummaryTests

/// "The panel header already names the version, so a leading title line
/// would be the same sentence twice."
fn drops_the_leading_title() {
    let summary = UpdateNotesSummary::summary(Some("# Upleft 1.1.0\n\n- Faster parsing"));
    assert!(!upleft_swift_text::contains(&summary, "Upleft 1.1.0"));
    assert!(upleft_swift_text::contains(&summary, "Faster parsing"));
}

fn stops_well_short_of_a_wall_of_text() {
    let long = (1..=60).map(|index| format!("- change number {index}")).collect::<Vec<_>>().join("\n");
    let summary = UpdateNotesSummary::summary(Some(&long));
    assert!(upleft_swift_text::contains(&summary, "change number 1"));
    assert!(!upleft_swift_text::contains(&summary, "change number 40"));
    assert!(upleft_swift_text::has_suffix(&summary, "…"), "a trimmed summary has to say that it was trimmed");
}

fn handles_no_notes_at_all() {
    assert!(UpdateNotesSummary::summary(None).is_empty());
    assert!(UpdateNotesSummary::summary(Some("   \n\n  ")).is_empty());
}

fn keeps_short_notes_whole() {
    let summary = UpdateNotesSummary::summary(Some("- one\n- two"));
    assert!(upleft_swift_text::str_eq(&summary, "- one\n- two"));
}

// MARK: - UpdateReleaseNotesReductionTests

/// "Linked release notes must never reach the WebKit-backed html document
/// type: the update channel's host is outside the app's documented network
/// surface, and a web engine would fetch remote subresources. The reduction
/// keeps the text, drops the machinery." (The fixture keeps the Swift
/// test's U+200B between the style and script elements.)
fn strips_tags_and_keeps_readable_text() {
    let html = "<html><head><style>body { color: red }</style>\u{200B}<script>alert(1)</script></head>\n\
                <body><h1>Upleft 1.0.17</h1><p>First &amp; second &#8212; line</p>\n\
                <ul><li>one</li><li>two</li></ul><p>Bye<br/>now</p></body></html>";
    let text = UpdateNotesView::text_from_html(html.as_bytes());
    assert!(upleft_swift_text::contains(&text, "Upleft 1.0.17"));
    assert!(upleft_swift_text::contains(&text, "First & second — line"));
    assert!(upleft_swift_text::contains(&text, "one") && upleft_swift_text::contains(&text, "two"));
    assert!(upleft_swift_text::contains(&text, "Bye\nnow"));
    assert!(
        !upleft_swift_text::contains(&upleft_swift_text::lowercased(&text), "alert"),
        "script content is dropped whole"
    );
    assert!(!upleft_swift_text::contains(&text, "{ color"), "style content is dropped whole");
    assert!(!upleft_swift_text::contains(&text, "<"), "no tags survive");
}

fn non_html_data_passes_through_as_text() {
    let markdown = "## Notes\n- plain";
    let view = UpdateNotesView::release_notes_text_view(
        markdown.as_bytes(),
        &Rc::new(StyleSheet::new(ThemeStore::shared().current(), &NSAppearance::currentDrawingAppearance(), Some(false))),
        mtm(),
    );
    assert!(upleft_swift_text::contains(&view.string().to_string(), "Notes"));
}

// MARK: - WindowChromeTests (the pill's part)

/// `documentToolbarKeepsTrailingControlsInOneCluster`: "the custom pill
/// must not draw NSButton's title", on a pill built outside the toolbar.
fn update_pill_draws_no_button_title() {
    let _ = NSApplication::sharedApplication(mtm());
    let update_pill = UpdateStatusPill::new_standard(mtm());
    assert!(update_pill.title().to_string().is_empty(), "the custom pill must not draw NSButton's title");
}

fn main() {
    let skipped = [(
        "WindowChromeTests/documentToolbarKeepsTrailingControlsInOneCluster (everything but the pill's title)",
        "needs DocumentWindowController's toolbar (app shell)",
    )];
    for (name, reason) in skipped {
        println!("test {name} ... skipped ({reason})");
    }
    main_thread::run(&[
        ("UpdatePanelFooterTests/rebuildsAfterCheckIntoAvailableState", rebuilds_after_check_into_available_state),
        ("UpdatePanelTransitionTests/rendersEveryUpdaterPhaseWithoutThrowing", renders_every_updater_phase_without_throwing),
        ("UpdateNotesSummaryTests/dropsTheLeadingTitle", drops_the_leading_title),
        ("UpdateNotesSummaryTests/stopsWellShortOfAWallOfText", stops_well_short_of_a_wall_of_text),
        ("UpdateNotesSummaryTests/handlesNoNotesAtAll", handles_no_notes_at_all),
        ("UpdateNotesSummaryTests/keepsShortNotesWhole", keeps_short_notes_whole),
        ("UpdateReleaseNotesReductionTests/stripsTagsAndKeepsReadableText", strips_tags_and_keeps_readable_text),
        ("UpdateReleaseNotesReductionTests/nonHTMLDataPassesThroughAsText", non_html_data_passes_through_as_text),
        (
            "WindowChromeTests/documentToolbarKeepsTrailingControlsInOneCluster (pill title)",
            update_pill_draws_no_button_title,
        ),
    ]);
}
