//! View-level tests for TableEditorView, TidySheetView and FrontMatterEditorView
//! (ported from DownrightAppTests: `TableEditorTests.swift`,
//! `FrontMatterEditorTests.swift`). Runs on the main thread; the one window
//! is borderless, placed at (-30000, -30000) and never ordered in or
//! activated.
//!
//! The support directory and the home folder are sandboxed
//! (`tests/document_support/mod.rs`), and `Preferences.shared` is a testing
//! instance installed before any panel is built: `PanelFont` reads it, and
//! the real one's load would publish the Quick Look appearance to the user's
//! global preferences.
//!
//! Every Swift test in the two files is ported. `PanelAccessibilityTests`
//! has no test of these panels.

mod document_support;
mod main_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAccessibility, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use upleft_app::panels::appkit_support::{accessibility_label, role};
use upleft_app::panels::front_matter_editor_view::{FrontMatterEditorDelegate, FrontMatterEditorView};
use upleft_app::panels::inspector_host_view::{InspectorHostView, InspectorSection};
use upleft_app::panels::panel_chrome::panel_surface_preferred_width;
use upleft_app::panels::table_editor_view::{TableEditorCell, TableEditorDelegate, TableEditorView};
use upleft_app::panels::tidy_sheet_view::{TidyProposal, TidySheetView};
use upleft_app::support::preferences::Preferences;
use upleft_core::NSRange;
use upleft_core::editing::front_matter_editing::FrontMatterEditOperation;
use upleft_core::editing::table_editing::{TableEditOperation, TableEditProposal};
use upleft_core::model::TableAlignment;
use upleft_core::parser::MarkdownParser;
use upleft_core::tidy::TidyDocument;
use upleft_render::theme::theme_store::ThemeStore;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main-thread tests run on the main thread")
}

// MARK: - TableEditorTests

const SOURCE: &str = "| Name | Count |\n| :--- | ---: |\n| Ada | 3 |\n| Grace | 4 |\n";

#[derive(Default)]
struct ProposalRecorder {
    proposal: RefCell<Option<TableEditProposal>>,
    source_range: Cell<Option<NSRange>>,
}

impl TableEditorDelegate for ProposalRecorder {
    fn table_editor_did_apply(&self, _editor: &TableEditorView, proposal: &TableEditProposal) {
        *self.proposal.borrow_mut() = Some(proposal.clone());
    }

    fn table_editor_did_request_source(&self, _editor: &TableEditorView, range: NSRange) {
        self.source_range.set(Some(range));
    }
}

fn attach(editor: &TableEditorView) -> Rc<ProposalRecorder> {
    let delegate = Rc::new(ProposalRecorder::default());
    let weak: Rc<dyn TableEditorDelegate> = delegate.clone();
    editor.set_delegate(Some(Rc::downgrade(&weak)));
    delegate
}

fn loads_rows_columns_and_source_range() {
    let editor = TableEditorView::new_default(MarkdownParser::parse(SOURCE), mtm());
    assert_eq!(editor.row_count_for_testing(), 3);
    assert_eq!(editor.column_count_for_testing(), 2);
    assert!(editor.source_range_for_testing().length > 0);
}

fn forwards_cell_edit_as_one_source_proposal() {
    let editor = TableEditorView::new_default(MarkdownParser::parse(SOURCE), mtm());
    let delegate = attach(&editor);
    editor.apply(&TableEditOperation::SetCell { row: 1, column: 0, text: "Ada Lovelace".into() });
    let proposal = delegate.proposal.borrow().clone().expect("a proposal");
    assert_eq!(proposal.summary, "Edit table cell");
    assert!(proposal.applying(SOURCE).is_some_and(|text| text.contains("Ada Lovelace")));
}

fn forwards_structure_and_alignment_operations() {
    let editor = TableEditorView::new_default(MarkdownParser::parse(SOURCE), mtm());
    let delegate = attach(&editor);

    editor.apply(&TableEditOperation::InsertColumn { index: 1, header: "Tag".into(), cells: vec!["a".into(), "b".into()] });
    assert_eq!(delegate.proposal.borrow().as_ref().expect("a proposal").summary, "Insert table column");

    editor.apply(&TableEditOperation::SetAlignment { column: 0, alignment: TableAlignment::Center });
    assert_eq!(delegate.proposal.borrow().as_ref().expect("a proposal").summary, "Set table alignment");
}

fn host_application_uses_one_undo_step() {
    let document = document_support::document();
    document.adopt(SOURCE, None);
    let editor = TableEditorView::new_default(document.parsed(), mtm());
    let delegate = attach(&editor);
    editor.apply(&TableEditOperation::SetCell { row: 1, column: 1, text: "5".into() });
    let proposal = delegate.proposal.borrow().clone().expect("a proposal");
    assert!(document.replace(proposal.range, &proposal.replacement, Some(proposal.summary.as_str())));
    assert!(document.text().contains("| 5 |"));
    assert!(document.undo_manager().canUndo());
    document.undo_manager().undo();
    assert_eq!(document.text(), SOURCE);
}

fn source_request_uses_whole_table_range() {
    let editor = TableEditorView::new_default(MarkdownParser::parse(SOURCE), mtm());
    let delegate = attach(&editor);
    editor.request_source_for_testing();
    assert_eq!(delegate.source_range.get(), Some(editor.source_range_for_testing()));
}

// MARK: - FrontMatterEditorTests

#[derive(Default)]
struct FrontMatterEditorSpy {
    editor: RefCell<Option<objc2::rc::Weak<FrontMatterEditorView>>>,
    did_request_source_mode: Cell<bool>,
}

impl FrontMatterEditorDelegate for FrontMatterEditorSpy {
    fn front_matter_editor_did_request(&self, _editor: &FrontMatterEditorView, _operation: FrontMatterEditOperation) {}

    fn front_matter_editor_wants_source_mode(&self, _editor: &FrontMatterEditorView) {
        self.did_request_source_mode.set(true);
    }
}

impl FrontMatterEditorSpy {
    fn editor_wants_source_mode(&self) {
        let Some(editor) = self.editor.borrow().as_ref().and_then(|editor| editor.load()) else { return };
        self.front_matter_editor_wants_source_mode(&editor);
    }
}

fn renders_flat_fields_and_uses_accessible_labels() {
    let editor = FrontMatterEditorView::new_current(mtm());
    editor.set_document(MarkdownParser::parse("---\ntitle: Demo\ndraft: false\n---\n\n# Body\n"));

    assert_eq!(editor.rendered_field_count(), 2);
    assert!(!editor.shows_source_mode_prompt());
    assert_eq!(editor.accessibilityRole().as_deref(), Some(role::group()));
    assert_eq!(accessibility_label(&*editor).as_deref(), Some("Front matter editor"));
}

fn complex_yaml_shows_source_mode_prompt() {
    let editor = FrontMatterEditorView::new_current(mtm());
    editor.set_document(MarkdownParser::parse("---\ntags:\n  - one\n  - two\n---\nBody\n"));

    assert_eq!(editor.rendered_field_count(), 1);
    assert!(editor.shows_source_mode_prompt());
}

fn missing_front_matter_disables_add_form() {
    let editor = FrontMatterEditorView::new_current(mtm());
    editor.set_document(MarkdownParser::parse("# Body\n"));

    assert_eq!(editor.rendered_field_count(), 0);
    assert!(editor.shows_source_mode_prompt());
}

fn editor_delegate_receives_source_mode_request() {
    let editor = FrontMatterEditorView::new_current(mtm());
    let delegate = Rc::new(FrontMatterEditorSpy::default());
    let weak: Rc<dyn FrontMatterEditorDelegate> = delegate.clone();
    editor.set_delegate(Some(Rc::downgrade(&weak)));
    *delegate.editor.borrow_mut() = Some(objc2::rc::Weak::from(&*editor));

    delegate.editor_wants_source_mode();
    assert!(delegate.did_request_source_mode.get());
}

/// Swift puts the editor in a titled window it never orders in. Here the
/// window is borderless (AppKit never constrains it onto a screen), at
/// (-30000, -30000), and never ordered in either; first responder and
/// layout need neither.
fn presentation_returns_to_top_and_focuses_the_requested_value() {
    let editor = FrontMatterEditorView::new_current(mtm());
    editor.set_document(MarkdownParser::parse(
        "---\ntitle: Demo\nauthor: Downright\ntags: one\nstatus: draft\n---\n\n# Body\n",
    ));
    let window = off_screen_window(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(340.0, 320.0)));
    window.setContentView(Some(&editor));
    window.layoutIfNeeded();
    editor.set_field_scroll_origin_y_for_testing(120.0);

    editor.prepare_for_presentation(Some("author"));

    assert!(editor.fittingSize().height >= 230.0);
    assert_eq!(editor.field_scroll_origin_y_for_testing(), 0.0);
    assert_eq!(editor.focused_field_key_for_testing().as_deref(), Some("author"));
    assert!(!window.isVisible());
    window.setContentView(None);
}

fn off_screen_window(content: NSRect) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm()),
            content,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
    window
}

// MARK: - Rust-only checks (no Swift test covers these)

/// The layout dumps compare class names, private classes included.
fn objective_c_class_names_match_swift() {
    let _ = TableEditorView::new_default(MarkdownParser::parse(SOURCE), mtm());
    // The table makes its cells only when it lays out rows in a window.
    let _ = TableEditorCell::with_string("cell", mtm());
    let tidy = TidySheetView::new_current(mtm());
    let text = "# A\n\n### B\n\n- x\n* y\n";
    let parsed = MarkdownParser::parse(text);
    let utf16: Vec<u16> = text.encode_utf16().collect();
    tidy.set_proposals(
        TidyDocument::plan(&parsed)
            .into_iter()
            .map(|edit| TidyProposal {
                before: String::from_utf16(&utf16[edit.range.location as usize..edit.range.upper_bound() as usize]).unwrap(),
                after: edit.replacement.clone(),
                edit,
            })
            .collect(),
    );
    tidy.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(620.0, 460.0)));
    tidy.layoutSubtreeIfNeeded();
    let editor = FrontMatterEditorView::new_current(mtm());
    editor.set_document(MarkdownParser::parse("---\ntitle: Demo\n---\n"));
    for name in [
        c"TableEditorView",
        c"TableEditorCell",
        c"TidySheetView",
        c"TidyGroupRowView",
        c"TidyProposalRowView",
        c"FrontMatterEditorView",
        c"FrontMatterFieldHost",
        c"FrontMatterFieldRow",
        c"FrontMatterDirtyDot",
    ] {
        assert!(AnyClass::get(name).is_some(), "{name:?} is not registered");
    }
}

/// `InspectorHostView` reaches the front matter editor by selector, and
/// `FloatingPanelSurface` reads `preferredWidth` from every `PanelSurface`.
fn panel_surface_selectors() {
    let table = TableEditorView::new_default(MarkdownParser::parse(SOURCE), mtm());
    let front = FrontMatterEditorView::new_current(mtm());
    let tidy = TidySheetView::new_current(mtm());
    assert_eq!(panel_surface_preferred_width(&table), Some(520.0));
    assert_eq!(panel_surface_preferred_width(&front), Some(336.0));
    assert_eq!(panel_surface_preferred_width(&tidy), None);

    front.set_document(MarkdownParser::parse("---\ntitle: Demo\nauthor: me\n---\n"));
    let host = InspectorHostView::new(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(336.0, 480.0)), mtm());
    let window = off_screen_window(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(336.0, 480.0)));
    window.setContentView(Some(&host));
    let content: &NSView = &front;
    host.set_content(content, InspectorSection::Context);
    window.layoutIfNeeded();
    host.focus_for_presentation();
    assert_eq!(front.focused_field_key_for_testing().as_deref(), Some("title"));
    assert!(!window.isVisible());
    window.setContentView(None);
}

/// `TidySheetView.displayText`: whitespace-only edits are described, and
/// trailing blanks shown as middle dots (ICU's `$` also matches before a
/// final line terminator, and a CR LF is one Character).
fn tidy_display_text_describes_whitespace() {
    assert_eq!(TidySheetView::display_text(""), "(nothing)");
    assert_eq!(TidySheetView::display_text(" "), "(1 space)");
    assert_eq!(TidySheetView::display_text("   "), "(3 spaces)");
    assert_eq!(TidySheetView::display_text("\n\n"), "(2 blank lines)");
    assert_eq!(TidySheetView::display_text("\r\n"), "(1 blank line)");
    assert_eq!(TidySheetView::display_text("abc  "), "abc··");
    assert_eq!(TidySheetView::display_text("line  \nnext\t\n"), "line··\nnext·\n");
    assert_eq!(TidySheetView::display_text("abc  \r\n"), "abc\r\n··");
}

/// Points the home folder into the sandbox and installs the testing
/// `Preferences.shared`. Call first thing in `main`.
fn sandbox() {
    let root = document_support::sandbox();
    let home = root.appending_path_component_is_directory("home", true);
    std::fs::create_dir_all(home.path()).unwrap();
    // SAFETY: called from `main` before any other thread exists.
    unsafe {
        std::env::set_var("CFFIXED_USER_HOME", home.path());
        std::env::set_var("HOME", home.path());
    }
    let preferences = Preferences::for_testing(root.appending_path_component("shared-preferences.json"), None);
    assert!(Preferences::install_shared_for_testing(preferences), "Preferences.shared was loaded too early");
    let themes = ThemeStore::user_themes_directory().and_then(|url| url.path()).unwrap().to_string();
    assert!(themes.starts_with(&home.path()), "the themes folder {themes} escaped the sandbox");
}

fn main() {
    sandbox();
    main_thread::run(&[
        ("loads_rows_columns_and_source_range", loads_rows_columns_and_source_range),
        ("forwards_cell_edit_as_one_source_proposal", forwards_cell_edit_as_one_source_proposal),
        ("forwards_structure_and_alignment_operations", forwards_structure_and_alignment_operations),
        ("host_application_uses_one_undo_step", host_application_uses_one_undo_step),
        ("source_request_uses_whole_table_range", source_request_uses_whole_table_range),
        ("renders_flat_fields_and_uses_accessible_labels", renders_flat_fields_and_uses_accessible_labels),
        ("complex_yaml_shows_source_mode_prompt", complex_yaml_shows_source_mode_prompt),
        ("missing_front_matter_disables_add_form", missing_front_matter_disables_add_form),
        ("editor_delegate_receives_source_mode_request", editor_delegate_receives_source_mode_request),
        (
            "presentation_returns_to_top_and_focuses_the_requested_value",
            presentation_returns_to_top_and_focuses_the_requested_value,
        ),
        ("objective_c_class_names_match_swift", objective_c_class_names_match_swift),
        ("panel_surface_selectors", panel_surface_selectors),
        ("tidy_display_text_describes_whitespace", tidy_display_text_describes_whitespace),
    ]);
    document_support::remove_sandbox();
}
