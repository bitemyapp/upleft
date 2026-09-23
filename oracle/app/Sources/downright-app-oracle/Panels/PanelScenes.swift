import AppKit
@testable import DownrightApp

/// Every panel scene, by the Swift type it builds (`"panel"` in a scenario).
/// One file per panel under `Scenes/`; the Rust registry is
/// `crates/conformance/src/dump/panel/mod.rs`.
enum PanelScenes {
    @MainActor
    static func make(_ name: String) throws -> PanelScene {
        switch name {
        case "ActivityIndicatorView": return ActivityIndicatorViewScene()
        case "AssetDoctorView": return AssetDoctorViewScene()
        case "BreadcrumbView": return BreadcrumbViewScene()
        case "ChangeSummaryBarView": return ChangeSummaryBarViewScene()
        case "ChromeGlass": return ChromeGlassScene()
        case "CommandPaletteView": return CommandPaletteViewScene()
        case "ConflictBarView": return ConflictBarViewScene()
        case "DocumentHealthView": return DocumentHealthViewScene()
        case "DocumentLensView": return DocumentLensViewScene()
        case "DocumentQuickLook": return DocumentQuickLookScene()
        case "DocumentStatusBarView": return DocumentStatusBarViewScene()
        case "FindBarView": return FindBarViewScene()
        case "FloatingPanelSurface": return FloatingPanelSurfaceScene()
        case "FrontMatterEditorView": return FrontMatterEditorViewScene()
        case "HistoryInspectorView": return HistoryInspectorViewScene()
        case "InspectorHostView": return InspectorHostViewScene()
        case "LightboxWindow": return LightboxWindowScene()
        case "LocalAIPanelView": return LocalAIPanelViewScene()
        case "PanelChrome": return PanelChromeScene()
        case "ReaderProfilePickerView": return ReaderProfilePickerViewScene()
        case "RenderTargetsView": return RenderTargetsViewScene()
        case "ReviewPanelView": return ReviewPanelViewScene()
        case "SearchInspectorView": return SearchInspectorViewScene()
        case "SearchResultsPanelView": return SearchResultsPanelViewScene()
        case "TableEditorView": return TableEditorViewScene()
        case "TaskPanelView": return TaskPanelViewScene()
        case "TaskProgressRing": return TaskProgressRingScene()
        case "TaskSectionBarView": return TaskSectionBarViewScene()
        case "TidySheetView": return TidySheetViewScene()
        case "TrustPromptView": return TrustPromptViewScene()
        case "UpdateNotesPopover": return UpdateNotesPopoverScene()
        case "UpdateStatusPill": return UpdateStatusPillScene()
        case "UpdateWindowController": return UpdateWindowControllerScene()
        case "VersionTimelineView": return VersionTimelineViewScene()
        case "VisualDebuggerView": return VisualDebuggerViewScene()
        case "WorkspaceSidebarView": return WorkspaceSidebarViewScene()
        default: throw PanelHarnessError.unknownPanel(name)
        }
    }
}
