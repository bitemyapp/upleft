//! Every panel scene, by the Swift type it builds (`"panel"` in a
//! scenario); one module per Swift panel, mirroring
//! `oracle/app/.../Panels/Scenes/*Scene.swift`.

use super::PanelScene;
use crate::dump::Failure;

pub mod activity_indicator_view;
pub mod asset_doctor_view;
pub mod breadcrumb_view;
pub mod change_summary_bar_view;
pub mod chrome_glass;
pub mod command_palette_view;
pub mod conflict_bar_view;
pub mod document_health_view;
pub mod document_lens_view;
pub mod document_quick_look;
pub mod document_status_bar_view;
pub mod find_bar_view;
pub mod floating_panel_surface;
pub mod front_matter_editor_view;
pub mod history_inspector_view;
pub mod inspector_host_view;
pub mod lightbox_window;
pub mod local_ai_panel_view;
pub mod panel_chrome;
pub mod reader_profile_picker_view;
pub mod render_targets_view;
pub mod review_panel_view;
pub mod search_inspector_view;
pub mod search_results_panel_view;
pub mod table_editor_view;
pub mod task_panel_view;
pub mod task_progress_ring;
pub mod task_section_bar_view;
pub mod tidy_sheet_view;
pub mod trust_prompt_view;
pub mod update_notes_popover;
pub mod update_status_pill;
pub mod update_window_controller;
pub mod version_timeline_view;
pub mod visual_debugger_view;
pub mod workspace_sidebar_view;

/// `PanelScenes.make(_:)`.
pub fn make(name: &str) -> Result<Box<dyn PanelScene>, Failure> {
    let scene: Box<dyn PanelScene> = match name {
        "ActivityIndicatorView" => Box::new(activity_indicator_view::ActivityIndicatorViewScene::default()),
        "AssetDoctorView" => Box::new(asset_doctor_view::AssetDoctorViewScene::default()),
        "BreadcrumbView" => Box::new(breadcrumb_view::BreadcrumbViewScene::default()),
        "ChangeSummaryBarView" => Box::new(change_summary_bar_view::ChangeSummaryBarViewScene::default()),
        "ChromeGlass" => Box::new(chrome_glass::ChromeGlassScene::default()),
        "CommandPaletteView" => Box::new(command_palette_view::CommandPaletteViewScene::default()),
        "ConflictBarView" => Box::new(conflict_bar_view::ConflictBarViewScene::default()),
        "DocumentHealthView" => Box::new(document_health_view::DocumentHealthViewScene::default()),
        "DocumentLensView" => Box::new(document_lens_view::DocumentLensViewScene::default()),
        "DocumentQuickLook" => Box::new(document_quick_look::DocumentQuickLookScene::default()),
        "DocumentStatusBarView" => Box::new(document_status_bar_view::DocumentStatusBarViewScene::default()),
        "FindBarView" => Box::new(find_bar_view::FindBarViewScene::default()),
        "FloatingPanelSurface" => Box::new(floating_panel_surface::FloatingPanelSurfaceScene::default()),
        "FrontMatterEditorView" => Box::new(front_matter_editor_view::FrontMatterEditorViewScene::default()),
        "HistoryInspectorView" => Box::new(history_inspector_view::HistoryInspectorViewScene::default()),
        "InspectorHostView" => Box::new(inspector_host_view::InspectorHostViewScene::default()),
        "LightboxWindow" => Box::new(lightbox_window::LightboxWindowScene::default()),
        "LocalAIPanelView" => Box::new(local_ai_panel_view::LocalAIPanelViewScene::default()),
        "PanelChrome" => Box::new(panel_chrome::PanelChromeScene::default()),
        "ReaderProfilePickerView" => Box::new(reader_profile_picker_view::ReaderProfilePickerViewScene::default()),
        "RenderTargetsView" => Box::new(render_targets_view::RenderTargetsViewScene::default()),
        "ReviewPanelView" => Box::new(review_panel_view::ReviewPanelViewScene::default()),
        "SearchInspectorView" => Box::new(search_inspector_view::SearchInspectorViewScene::default()),
        "SearchResultsPanelView" => Box::new(search_results_panel_view::SearchResultsPanelViewScene::default()),
        "TableEditorView" => Box::new(table_editor_view::TableEditorViewScene::default()),
        "TaskPanelView" => Box::new(task_panel_view::TaskPanelViewScene::default()),
        "TaskProgressRing" => Box::new(task_progress_ring::TaskProgressRingScene::default()),
        "TaskSectionBarView" => Box::new(task_section_bar_view::TaskSectionBarViewScene::default()),
        "TidySheetView" => Box::new(tidy_sheet_view::TidySheetViewScene::default()),
        "TrustPromptView" => Box::new(trust_prompt_view::TrustPromptViewScene::default()),
        "UpdateNotesPopover" => Box::new(update_notes_popover::UpdateNotesPopoverScene::default()),
        "UpdateStatusPill" => Box::new(update_status_pill::UpdateStatusPillScene::default()),
        "UpdateWindowController" => Box::new(update_window_controller::UpdateWindowControllerScene::default()),
        "VersionTimelineView" => Box::new(version_timeline_view::VersionTimelineViewScene::default()),
        "VisualDebuggerView" => Box::new(visual_debugger_view::VisualDebuggerViewScene::default()),
        "WorkspaceSidebarView" => Box::new(workspace_sidebar_view::WorkspaceSidebarViewScene::default()),
        other => return Err(Failure::Error(format!("unknown panel {other}"))),
    };
    Ok(scene)
}
