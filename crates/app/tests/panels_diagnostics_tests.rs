//! View-level tests for DocumentHealthView, RenderTargetsView, AssetDoctorView, DocumentLensView, LightboxWindow, DocumentQuickLook
//! (ported from DownrightAppTests). Runs on the main thread; any window is
//! off-screen and never activated.
//!
//! Ported:
//! - `DiagnosticsViewTests.swift`: all five tests.
//! - `AssetDoctorViewTests.swift`: all three tests.
//! - `DocumentLensTests.swift`: `DocumentLensViewTests.tabSwitchAndReturnSelectsTheExactItem`
//!   (`DocumentLensModelTests` is `tests/document_lens_tests.rs`).
//! - `DocumentQuickLookTests.swift`: the seven routing tests and
//!   `noCommandBindsAnUnmodifiedSpace`.
//!
//! Skipped (and why):
//! - `DocumentQuickLookTests.quickLookIsAFileMenuCommandOnTheStandardChord`
//!   and `quickLookIsOfferedOnlyWithATargetUnderTheCaret`: already ported, in
//!   `tests/document_quick_look_tests_menu.rs` and
//!   `tests/document_quick_look_tests_commands.rs`.
//! - `DocumentQuickLookTests.theCommandFollowsTheCaretAndReportsWhenThereIsNothingToShow`:
//!   builds a `DocumentWindowController` (App/, not ported on this branch).
//!   Its Quick Look half is `QuickLookHost`, which the controller implements.
//! - `PanelAccessibilityTests.swift` names none of these panels.
//!
//! No Swift test covers `LightboxWindow`; the `panel` and `panel-model`
//! suites do.

#[path = "document_support/mod.rs"]
mod document_support;
#[path = "main_thread/mod.rs"]
mod main_thread;

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSTableView, NSView};
use upleft_app::assets::asset_doctor::{
    AssetDiagnostic, AssetDiagnosticCode, AssetDiagnosticSeverity, AssetDoctor, AssetProposalKind, AssetSourceProposal,
};
use upleft_app::assets::asset_resolver::{AssetReference, AssetReferenceKind};
use upleft_app::ai::path_resolver::Resolution;
use upleft_app::lens::document_lens_model::{DocumentLensInput, DocumentLensItem, DocumentLensModel, DocumentLensTab};
use upleft_app::panels::appkit_support::accessibility_label;
use upleft_app::panels::asset_doctor_view::{AssetDoctorView, AssetDoctorViewDelegate};
use upleft_app::panels::document_health_view::{DocumentHealthView, DocumentHealthViewDelegate};
use upleft_app::panels::document_lens_view::{DocumentLensView, DocumentLensViewDelegate};
use upleft_app::panels::document_quick_look::QuickLookRequest;
use upleft_app::panels::render_targets_view::{RenderTargetsView, RenderTargetsViewDelegate};
use upleft_app::support::keybindings::KeybindingDefaults;
use upleft_core::compatibility::compatibility_diagnostics::CompatibilityDiagnostic;
use upleft_core::compatibility::render_target::{MarkdownCapabilities, RenderTargetProfile};
use upleft_core::contracts::TextEdit;
use upleft_core::health::document_health::{DocumentHealth, DocumentHealthDiagnostic};
use upleft_core::parser::MarkdownParser;
use upleft_core::{NSRange, PathToken};
use upleft_foundation::url::FileUrl;
use upleft_render::view::markdown_text_view_delegate::{ContextTarget, ContextTargetKind};

fn main() {
    // The panels read `Preferences.shared` (`PanelFont`); keep it, and every
    // other shared store, out of the real home.
    document_support::sandbox();
    main_thread::run_off_screen(&[
        ("health_groups_findings_and_keeps_exact_ranges", health_groups_findings_and_keeps_exact_ranges),
        ("health_sends_only_safe_fixes_as_one_batch", health_sends_only_safe_fixes_as_one_batch),
        ("health_local_ignore_can_be_reset", health_local_ignore_can_be_reset),
        (
            "render_targets_accept_injected_custom_profile_and_expose_proposal",
            render_targets_accept_injected_custom_profile_and_expose_proposal,
        ),
        ("render_target_rows_expose_source_range_to_voice_over", render_target_rows_expose_source_range_to_voice_over),
        ("groups_diagnostics_and_reports_empty_state", groups_diagnostics_and_reports_empty_state),
        (
            "diagnostic_row_exposes_severity_and_source_to_accessibility",
            diagnostic_row_exposes_severity_and_source_to_accessibility,
        ),
        ("proposal_passes_through_delegate_without_mutating_source", proposal_passes_through_delegate_without_mutating_source),
        ("tab_switch_and_return_selects_the_exact_item", tab_switch_and_return_selects_the_exact_item),
        ("an_embedded_image_goes_to_the_apps_own_lightbox", an_embedded_image_goes_to_the_apps_own_lightbox),
        ("a_resolved_path_token_goes_to_the_system_panel", a_resolved_path_token_goes_to_the_system_panel),
        ("a_missing_path_token_is_not_previewable", a_missing_path_token_is_not_previewable),
        ("a_relative_link_resolves_against_the_documents_folder", a_relative_link_resolves_against_the_documents_folder),
        ("a_file_url_link_is_previewed_directly", a_file_url_link_is_previewed_directly),
        ("web_anchor_and_automation_links_are_never_previewed", web_anchor_and_automation_links_are_never_previewed),
        ("targets_with_no_file_behind_them_are_not_previewable", targets_with_no_file_behind_them_are_not_previewable),
        ("no_command_binds_an_unmodified_space", no_command_binds_an_unmodified_space),
    ]);
    document_support::remove_sandbox();
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main-thread tests run on the main thread")
}

// MARK: - The table data-source calls the Swift tests make

/// `view.numberOfRows(in: NSTableView())`.
fn number_of_rows(view: &AnyObject) -> isize {
    let table = NSTableView::new(mtm());
    unsafe { msg_send![view, numberOfRowsInTableView: &*table] }
}

/// `view.tableView(NSTableView(), isGroupRow: row)`.
fn is_group_row(view: &AnyObject, row: isize) -> bool {
    let table = NSTableView::new(mtm());
    unsafe { msg_send![view, tableView: &*table, isGroupRow: row] }
}

/// `view.tableView(NSTableView(), viewFor: nil, row: row)`.
fn view_for(view: &AnyObject, row: isize) -> Option<Retained<NSView>> {
    let table = NSTableView::new(mtm());
    unsafe { msg_send![view, tableView: &*table, viewForTableColumn: std::ptr::null::<AnyObject>(), row: row] }
}

fn row_label(view: &AnyObject, row: isize) -> Option<String> {
    view_for(view, row).and_then(|cell| accessibility_label(&*cell))
}

// MARK: - DiagnosticsViewTests

#[derive(Default)]
struct RecordingDiagnosticsDelegate {
    health_fixes: RefCell<Vec<TextEdit>>,
    target_fixes: RefCell<Vec<TextEdit>>,
}

impl DocumentHealthViewDelegate for RecordingDiagnosticsDelegate {
    fn document_health_view_did_select(&self, _view: &DocumentHealthView, _diagnostic: &DocumentHealthDiagnostic) {}
    fn document_health_view_did_apply(&self, _view: &DocumentHealthView, fixes: &[TextEdit]) {
        *self.health_fixes.borrow_mut() = fixes.to_vec();
    }
    fn document_health_view_wants_source_mode(&self, _view: &DocumentHealthView) {}
}

impl RenderTargetsViewDelegate for RecordingDiagnosticsDelegate {
    fn render_targets_view_did_select_profile(&self, _view: &RenderTargetsView, _profile: &RenderTargetProfile) {}
    fn render_targets_view_did_select_diagnostic(&self, _view: &RenderTargetsView, _diagnostic: &CompatibilityDiagnostic) {}
    fn render_targets_view_did_apply(&self, _view: &RenderTargetsView, fixes: &[TextEdit]) {
        *self.target_fixes.borrow_mut() = fixes.to_vec();
    }
    fn render_targets_view_wants_source_mode(&self, _view: &RenderTargetsView) {}
}

fn health_groups_findings_and_keeps_exact_ranges() {
    let source = "# Title\n\n#### Café\n";
    let finding = DocumentHealth::analyze(source)
        .into_iter()
        .find(|finding| finding.id == "heading.skipped-level")
        .expect("#require: a skipped-level finding");
    let view = DocumentHealthView::new_current(mtm());
    view.set_source_text(source);
    view.set_diagnostics(vec![finding]);

    assert_eq!(accessibility_label(&*view).as_deref(), Some("Document health"));
    assert_eq!(number_of_rows(&view), 2);
    assert!(is_group_row(&view, 0));
    let label = row_label(&view, 1);
    assert_eq!(label.map(|label| label.contains("Source range 9\u{2013}18")), Some(true));
}

fn health_sends_only_safe_fixes_as_one_batch() {
    let source = "# Title\n\n#### Café\n";
    let findings = DocumentHealth::analyze(source);
    let view = DocumentHealthView::new_current(mtm());
    let delegate = Rc::new(RecordingDiagnosticsDelegate::default());
    let weak: Weak<dyn DocumentHealthViewDelegate> = Rc::downgrade(&(delegate.clone() as Rc<dyn DocumentHealthViewDelegate>));
    view.set_delegate(Some(weak));
    view.set_source_text(source);
    view.set_diagnostics(findings);

    view.apply_safe_fixes_for_testing();

    assert_eq!(delegate.health_fixes.borrow().len(), 1);
    assert_eq!(delegate.health_fixes.borrow().first().map(|fix| fix.replacement.as_str()), Some("## "));
}

fn health_local_ignore_can_be_reset() {
    let source = "# Title\n\n#### Café\n";
    let finding = DocumentHealth::analyze(source)
        .into_iter()
        .find(|finding| finding.id == "heading.skipped-level")
        .expect("#require: a skipped-level finding");
    let view = DocumentHealthView::new_current(mtm());
    view.set_source_text(source);
    view.set_diagnostics(vec![finding]);
    view.select_finding_for_testing(1);
    view.ignore_selection_for_testing();
    assert_eq!(number_of_rows(&view), 0);
    // Reset is intentionally local. It does not change the source or the
    // report.
    view.reset_ignored_findings();
    assert_eq!(number_of_rows(&view), 2);
}

fn render_targets_accept_injected_custom_profile_and_expose_proposal() {
    let source = "See [[Design Notes|the notes]].\n";
    let document = MarkdownParser::parse(source);
    let custom = RenderTargetProfile::custom("Strict docs", MarkdownCapabilities::RAW_HTML);
    let view = RenderTargetsView::new_current(mtm());
    let delegate = Rc::new(RecordingDiagnosticsDelegate::default());
    let weak: Weak<dyn RenderTargetsViewDelegate> = Rc::downgrade(&(delegate.clone() as Rc<dyn RenderTargetsViewDelegate>));
    view.set_delegate(Some(weak));
    view.set_profiles(vec![custom.clone(), RenderTargetProfile::git_hub()]);
    view.set_selected_profile(custom.clone());
    view.set_source_text(source);
    view.set_document(document);

    assert_eq!(view.selected_profile(), custom);
    assert_eq!(number_of_rows(&view), 2);
    view.apply_safe_fixes_for_testing();
    assert_eq!(delegate.target_fixes.borrow().len(), 1);
    assert_eq!(
        delegate.target_fixes.borrow().first().map(|fix| fix.replacement.as_str()),
        Some("[the notes](<Design Notes>)")
    );
}

fn render_target_rows_expose_source_range_to_voice_over() {
    let source = "See [[Design Notes]].\n";
    let view = RenderTargetsView::new_current(mtm());
    view.set_source_text(source);
    view.set_document(MarkdownParser::parse(source));
    let label = row_label(&view, 1).expect("#require: a row label");
    assert!(label.contains("Source range"));
    assert!(label.contains("Wikilinks"));
}

// MARK: - AssetDoctorViewTests

#[derive(Default)]
struct RecordingAssetDoctorDelegate {
    selected: RefCell<Option<AssetDiagnostic>>,
    applied: RefCell<Option<AssetSourceProposal>>,
}

impl AssetDoctorViewDelegate for RecordingAssetDoctorDelegate {
    fn asset_doctor_view_did_select(&self, _view: &AssetDoctorView, diagnostic: &AssetDiagnostic) {
        *self.selected.borrow_mut() = Some(diagnostic.clone());
    }
    fn asset_doctor_view_did_reveal(&self, _view: &AssetDoctorView, _diagnostic: &AssetDiagnostic) {}
    fn asset_doctor_view_did_request_proposal(
        &self,
        _view: &AssetDoctorView,
        _kind: AssetProposalKind,
        _diagnostic: &AssetDiagnostic,
    ) {
    }
    fn asset_doctor_view_did_apply(&self, _view: &AssetDoctorView, proposal: &AssetSourceProposal) {
        *self.applied.borrow_mut() = Some(proposal.clone());
    }
}

fn asset_diagnostic(code: AssetDiagnosticCode, _severity: AssetDiagnosticSeverity, line: isize) -> AssetDiagnostic {
    let source = "assets/image.png";
    let reference = AssetReference {
        source: source.to_owned(),
        source_text: source.to_owned(),
        destination_range: NSRange::new(7, upleft_swift_text::utf16_count(source)),
        image_range: NSRange::new(0, 24),
        alt_text: if code == AssetDiagnosticCode::MissingAlt { String::new() } else { "Image".to_owned() },
        title: None,
        kind: AssetReferenceKind::RelativeLocal,
        url: Some(FileUrl::from_path("/tmp/assets/image.png")),
        line,
    };
    let range = reference.destination_range;
    AssetDiagnostic::new(code, format!("Asset diagnostic for {}.", code.raw_value()), range, reference)
}

fn groups_diagnostics_and_reports_empty_state() {
    let view = AssetDoctorView::new_current(mtm());
    assert_eq!(accessibility_label(&*view).as_deref(), Some("Asset Doctor"));
    assert_eq!(number_of_rows(&view), 0);

    view.set_diagnostics(vec![
        asset_diagnostic(AssetDiagnosticCode::Missing, AssetDiagnosticSeverity::Warning, 2),
        asset_diagnostic(AssetDiagnosticCode::Missing, AssetDiagnosticSeverity::Warning, 3),
        asset_diagnostic(AssetDiagnosticCode::Unsafe, AssetDiagnosticSeverity::Error, 8),
    ]);
    // One group and one row per diagnostic. Group order is stable by code.
    assert_eq!(number_of_rows(&view), 5);
    assert!(is_group_row(&view, 0));
    assert!(!is_group_row(&view, 1));
    assert!(is_group_row(&view, 3));
}

fn diagnostic_row_exposes_severity_and_source_to_accessibility() {
    let view = AssetDoctorView::new_current(mtm());
    view.set_diagnostics(vec![asset_diagnostic(AssetDiagnosticCode::MissingAlt, AssetDiagnosticSeverity::Warning, 14)]);
    let label = row_label(&view, 1).unwrap_or_default();
    assert!(label.contains("Warning"));
    assert!(label.contains("line 14"));
    assert!(label.contains("assets/image.png"));
}

fn proposal_passes_through_delegate_without_mutating_source() {
    let view = AssetDoctorView::new_current(mtm());
    let reference = asset_diagnostic(AssetDiagnosticCode::Missing, AssetDiagnosticSeverity::Warning, 1).reference;
    let diagnostic = AssetDiagnostic::new(
        AssetDiagnosticCode::Missing,
        "Image asset was not found.",
        reference.destination_range,
        reference,
    );
    let delegate = Rc::new(RecordingAssetDoctorDelegate::default());
    let weak: Weak<dyn AssetDoctorViewDelegate> = Rc::downgrade(&(delegate.clone() as Rc<dyn AssetDoctorViewDelegate>));
    view.set_delegate(Some(weak));
    let proposal = AssetDoctor::relink_proposal(&diagnostic.reference, "assets/new image.png");

    view.apply(&proposal);

    assert_eq!(
        delegate.applied.borrow().as_ref().map(|proposal| proposal.replacement.as_str()),
        Some("assets/new%20image.png")
    );
    assert!(delegate.selected.borrow().is_none());
}

// MARK: - DocumentLensViewTests

#[derive(Default)]
struct RecordingLensDelegate {
    range: RefCell<Option<NSRange>>,
    profile: RefCell<Option<RenderTargetProfile>>,
}

impl DocumentLensViewDelegate for RecordingLensDelegate {
    fn document_lens_did_select(&self, _view: &DocumentLensView, range: NSRange, _item: &DocumentLensItem) {
        *self.range.borrow_mut() = Some(range);
    }
    fn document_lens_did_select_render_target(&self, _view: &DocumentLensView, profile: &RenderTargetProfile) {
        *self.profile.borrow_mut() = Some(profile.clone());
    }
}

fn tab_switch_and_return_selects_the_exact_item() {
    let text = "# Title\n\n- [ ] Task\n";
    let document = MarkdownParser::parse(text);
    let view = DocumentLensView::new_current(mtm());
    view.set_model(DocumentLensModel::new(&DocumentLensInput::new(document.clone())));
    let delegate = Rc::new(RecordingLensDelegate::default());
    let weak: Weak<dyn DocumentLensViewDelegate> = Rc::downgrade(&(delegate.clone() as Rc<dyn DocumentLensViewDelegate>));
    view.set_delegate(Some(weak));

    view.set_selected_tab(DocumentLensTab::Tasks);
    assert_eq!(number_of_rows(&view), 2);
    let label = row_label(&view, 1);
    assert_eq!(label.map(|label| label.contains("Task")), Some(true));
    // The callback contract is exercised through the view's test hook.
    view.select_item_for_testing(1);
    assert_eq!(*delegate.range.borrow(), document.tasks.first().map(|task| task.content_range));
}

// MARK: - DocumentQuickLookTests: routing

/// `makeTemporaryDirectory()`: `FileManager.default.temporaryDirectory`
/// plus a unique folder, removed by the returned guard (Swift's `defer`).
struct TemporaryDirectory(FileUrl);

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.path());
    }
}

fn make_temporary_directory() -> TemporaryDirectory {
    // Swift names the folder with a fresh UUID; any unique name will do.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_nanos());
    let name = format!("DocumentQuickLookTests-{}-{nanos}", std::process::id());
    let path = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&path).expect("temporary directory");
    TemporaryDirectory(FileUrl::from_path_is_directory(&path.to_string_lossy(), true))
}

fn resolve(kind: ContextTargetKind, document_url: Option<&FileUrl>, resolution: Option<Resolution>) -> Option<QuickLookRequest> {
    QuickLookRequest::resolve(
        &ContextTarget::new(kind, NSRange::new(0, 0), None),
        document_url,
        &|_| resolution.clone(),
        &|url| upleft_foundation::file_manager::file_exists(&url.path()),
    )
}

/// An embedded image opens in the lightbox, not the system panel. A click
/// on that same image already opens the lightbox (§7.1), and force click
/// summoning a second, different image viewer for the same picture would be
/// two answers to one question.
fn an_embedded_image_goes_to_the_apps_own_lightbox() {
    assert_eq!(
        resolve(ContextTargetKind::Image("diagram.png".into()), None, None),
        Some(QuickLookRequest::Lightbox { source: "diagram.png".into() })
    );
}

fn a_resolved_path_token_goes_to_the_system_panel() {
    let directory = make_temporary_directory();
    let file = directory.0.appending_path_component("session.ts");
    std::fs::write(file.path(), "export const x = 1\n").expect("write");

    let request = resolve(
        ContextTargetKind::PathToken(PathToken::new("src/session.ts", None, None)),
        None,
        Some(Resolution { url: Some(file.clone()), exists: true, is_directory: false, line: None }),
    );
    assert_eq!(request, Some(QuickLookRequest::Panel(file.standardized_file_url())));
}

/// §8.4 draws a path that is not there as missing. Previewing it would be a
/// second, contradictory answer about the same file.
fn a_missing_path_token_is_not_previewable() {
    assert_eq!(
        resolve(
            ContextTargetKind::PathToken(PathToken::new("src/gone.ts", None, None)),
            None,
            Some(Resolution {
                url: Some(FileUrl::from_path("/nope/gone.ts")),
                exists: false,
                is_directory: false,
                line: None,
            }),
        ),
        None
    );
    assert_eq!(resolve(ContextTargetKind::PathToken(PathToken::new("src/gone.ts", None, None)), None, None), None);
}

fn a_relative_link_resolves_against_the_documents_folder() {
    let directory = make_temporary_directory();
    let document_url = directory.0.appending_path_component("notes.md");
    let sibling = directory.0.appending_path_component("design.md");
    std::fs::write(sibling.path(), "# Design\n").expect("write");

    assert_eq!(
        resolve(ContextTargetKind::Link("design.md".into()), Some(&document_url), None),
        Some(QuickLookRequest::Panel(sibling.standardized_file_url()))
    );
    // Nothing to resolve against, and no guessing.
    assert_eq!(resolve(ContextTargetKind::Link("design.md".into()), None, None), None);
    assert_eq!(resolve(ContextTargetKind::Link("missing.md".into()), Some(&document_url), None), None);
}

fn a_file_url_link_is_previewed_directly() {
    let directory = make_temporary_directory();
    let sibling = directory.0.appending_path_component("spec.pdf");
    std::fs::write(sibling.path(), b"%PDF-1.4\n").expect("write");
    assert_eq!(
        resolve(ContextTargetKind::Link(sibling.absolute_string()), None, None),
        Some(QuickLookRequest::Panel(sibling.standardized_file_url()))
    );
}

/// Turning a preview gesture into "launch this URL" would route around the
/// trust prompt a real click goes through.
fn web_anchor_and_automation_links_are_never_previewed() {
    assert_eq!(resolve(ContextTargetKind::Link("https://example.com".into()), None, None), None);
    assert_eq!(resolve(ContextTargetKind::Link("#a-heading".into()), None, None), None);
    assert_eq!(resolve(ContextTargetKind::Link("x-downright://run".into()), None, None), None);
    assert_eq!(resolve(ContextTargetKind::Link(String::new()), None, None), None);
}

fn targets_with_no_file_behind_them_are_not_previewable() {
    assert_eq!(resolve(ContextTargetKind::Heading(0), None, None), None);
    assert_eq!(resolve(ContextTargetKind::CodeBlock(NSRange::new(0, 4)), None, None), None);
    assert_eq!(resolve(ContextTargetKind::Table(NSRange::new(0, 4)), None, None), None);
    assert_eq!(resolve(ContextTargetKind::Selection, None, None), None);
    assert_eq!(resolve(ContextTargetKind::Plain, None, None), None);
}

// MARK: - DocumentQuickLookTests: the trigger

/// The one that matters. A menu item carrying a bare Space as its key
/// equivalent intercepts the space bar before the text view ever sees it,
/// and `applyKeyEquivalent` only refuses bindings without ⌘ or ⌃ — so the
/// guard has to hold in the table too, not just in the menu builder.
fn no_command_binds_an_unmodified_space() {
    for (command, bindings) in KeybindingDefaults::table() {
        for binding in bindings.iter().filter(|binding| binding.key == "space") {
            assert!(
                !binding.modifiers.is_empty(),
                "{} binds a bare Space, which is a character in Live and Source",
                command.raw_value()
            );
        }
    }
}
