//! Port of `Sources/DownrightApp/Support/Commands.swift`.
//!
//! Every command in the app, in one declarative table (§7.2).
//!
//! The menu bar, the keybinding editor, the command palette, and the context
//! menus are all *derived* from this table. That is the point: a binding you
//! can see in the menu is by construction the binding the keyboard layer
//! dispatches, and adding a command in one place makes it remappable, listed,
//! and discoverable without touching four files.
//!
//! Everything here is data or pure computation. Running a command is the
//! window controller's job (`DocumentWindowController+Commands.swift`, ported
//! with the UI); it receives a [`Command`] and switches over it.

use objc2_app_kit::NSEvent;
use upleft_foundation::json_decoder::{self, DecodingError, Value};
use upleft_foundation::json_encoder::JsonValue;
use upleft_swift_text as swift_text;

/// `enum Command: String, CaseIterable, Codable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Command {
    SourceMode,
    SplitView,
    PinWindow,
    FocusMode,
    TypewriterScrolling,
    StatusBar,
    TaskPanel,
    VersionTimeline,
    CompareFiles,
    FrontMatterEditor,
    TableEditor,
    AssetDoctor,
    CommandPalette,
    DocumentLens,
    ReaderProfiles,
    DocumentHealth,
    RenderTargets,
    VisualDebugger,
    ReviewPanel,
    Workspace,
    LocalAi,
    NextHeading,
    PreviousHeading,
    NextChange,
    PreviousChange,
    MarkChangesReviewed,
    FollowLinkAtCaret,
    NextLink,
    PreviousLink,
    ScrollDown,
    ScrollUp,
    PageDown,
    PageUp,
    DocumentStart,
    DocumentEnd,
    GoBack,
    GoForward,
    ZoomLevel1,
    ZoomLevel2,
    ZoomLevel3,
    ZoomLevel4,
    ZoomLevel5,
    ZoomIn,
    ZoomOut,
    Find,
    FindNext,
    FindPrevious,
    FindReplace,
    FindInSiblings,
    UseSelectionForFind,
    PromoteHeading,
    DemoteHeading,
    HeadingLevel1,
    HeadingLevel2,
    HeadingLevel3,
    HeadingLevel4,
    HeadingLevel5,
    HeadingLevel6,
    HeadingToBody,
    MoveBlockUp,
    MoveBlockDown,
    FoldSection,
    UnfoldSection,
    FoldAll,
    UnfoldAll,
    ConvertToParagraph,
    ConvertToBulletList,
    ConvertToNumberedList,
    ConvertToTaskList,
    ConvertToBlockquote,
    SortListAlphabetically,
    SortListByState,
    InsertTableOfContents,
    TidyDocument,
    ToggleBold,
    ToggleItalic,
    InsertLink,
    ToggleStrikethrough,
    ToggleInlineCode,
    IndentList,
    OutdentList,
    ToggleTaskAtCaret,
    NewDocument,
    Open,
    Save,
    SaveAs,
    RevealInFinder,
    OpenInEditor,
    Close,
    QuickLook,
    CopyAsMarkdown,
    CopyAsRichText,
    CopyAsPlainText,
    CopySection,
    CopySectionLink,
    PrintDocument,
    ExportHtml,
    ExportPdf,
    ExportSelectionAsImage,
    Share,
    ShareAsPdf,
    IncreaseTextSize,
    DecreaseTextSize,
    ResetTextSize,
    SpeakDocument,
    StopSpeaking,
    Preferences,
    ReloadTheme,
    ShowKeybindings,
    CheckForUpdates,
    ToggleLightDark,
    GoToLine,
}

/// Swift's `==` on strings: canonical equivalence. Every raw value and
/// literal compared against here is ASCII, which is its own NFC.
fn swift_eq(value: &str, literal: &str) -> bool {
    value == literal || (!value.is_ascii() && swift_text::str_eq(value, literal))
}

/// `value` as the ASCII key a `switch` over string literals would match, or
/// `None` when no ASCII literal can equal it.
fn ascii_key(value: &str) -> Option<std::borrow::Cow<'_, str>> {
    if value.is_ascii() {
        return Some(std::borrow::Cow::Borrowed(value));
    }
    let normalized = swift_text::nfc(value);
    normalized.is_ascii().then_some(std::borrow::Cow::Owned(normalized))
}

impl Command {
    /// `Command.allCases`, in declaration order.
    pub const ALL_CASES: [Command; 112] = [
        Command::SourceMode,
        Command::SplitView,
        Command::PinWindow,
        Command::FocusMode,
        Command::TypewriterScrolling,
        Command::StatusBar,
        Command::TaskPanel,
        Command::VersionTimeline,
        Command::CompareFiles,
        Command::FrontMatterEditor,
        Command::TableEditor,
        Command::AssetDoctor,
        Command::CommandPalette,
        Command::DocumentLens,
        Command::ReaderProfiles,
        Command::DocumentHealth,
        Command::RenderTargets,
        Command::VisualDebugger,
        Command::ReviewPanel,
        Command::Workspace,
        Command::LocalAi,
        Command::NextHeading,
        Command::PreviousHeading,
        Command::NextChange,
        Command::PreviousChange,
        Command::MarkChangesReviewed,
        Command::FollowLinkAtCaret,
        Command::NextLink,
        Command::PreviousLink,
        Command::ScrollDown,
        Command::ScrollUp,
        Command::PageDown,
        Command::PageUp,
        Command::DocumentStart,
        Command::DocumentEnd,
        Command::GoBack,
        Command::GoForward,
        Command::ZoomLevel1,
        Command::ZoomLevel2,
        Command::ZoomLevel3,
        Command::ZoomLevel4,
        Command::ZoomLevel5,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::Find,
        Command::FindNext,
        Command::FindPrevious,
        Command::FindReplace,
        Command::FindInSiblings,
        Command::UseSelectionForFind,
        Command::PromoteHeading,
        Command::DemoteHeading,
        Command::HeadingLevel1,
        Command::HeadingLevel2,
        Command::HeadingLevel3,
        Command::HeadingLevel4,
        Command::HeadingLevel5,
        Command::HeadingLevel6,
        Command::HeadingToBody,
        Command::MoveBlockUp,
        Command::MoveBlockDown,
        Command::FoldSection,
        Command::UnfoldSection,
        Command::FoldAll,
        Command::UnfoldAll,
        Command::ConvertToParagraph,
        Command::ConvertToBulletList,
        Command::ConvertToNumberedList,
        Command::ConvertToTaskList,
        Command::ConvertToBlockquote,
        Command::SortListAlphabetically,
        Command::SortListByState,
        Command::InsertTableOfContents,
        Command::TidyDocument,
        Command::ToggleBold,
        Command::ToggleItalic,
        Command::InsertLink,
        Command::ToggleStrikethrough,
        Command::ToggleInlineCode,
        Command::IndentList,
        Command::OutdentList,
        Command::ToggleTaskAtCaret,
        Command::NewDocument,
        Command::Open,
        Command::Save,
        Command::SaveAs,
        Command::RevealInFinder,
        Command::OpenInEditor,
        Command::Close,
        Command::QuickLook,
        Command::CopyAsMarkdown,
        Command::CopyAsRichText,
        Command::CopyAsPlainText,
        Command::CopySection,
        Command::CopySectionLink,
        Command::PrintDocument,
        Command::ExportHtml,
        Command::ExportPdf,
        Command::ExportSelectionAsImage,
        Command::Share,
        Command::ShareAsPdf,
        Command::IncreaseTextSize,
        Command::DecreaseTextSize,
        Command::ResetTextSize,
        Command::SpeakDocument,
        Command::StopSpeaking,
        Command::Preferences,
        Command::ReloadTheme,
        Command::ShowKeybindings,
        Command::CheckForUpdates,
        Command::ToggleLightDark,
        Command::GoToLine
    ];

    /// `rawValue`.
    pub fn raw_value(self) -> &'static str {
        match self {
            Command::SourceMode => "sourceMode",
            Command::SplitView => "splitView",
            Command::PinWindow => "pinWindow",
            Command::FocusMode => "focusMode",
            Command::TypewriterScrolling => "typewriterScrolling",
            Command::StatusBar => "statusBar",
            Command::TaskPanel => "taskPanel",
            Command::VersionTimeline => "versionTimeline",
            Command::CompareFiles => "compareFiles",
            Command::FrontMatterEditor => "frontMatterEditor",
            Command::TableEditor => "tableEditor",
            Command::AssetDoctor => "assetDoctor",
            Command::CommandPalette => "commandPalette",
            Command::DocumentLens => "documentLens",
            Command::ReaderProfiles => "readerProfiles",
            Command::DocumentHealth => "documentHealth",
            Command::RenderTargets => "renderTargets",
            Command::VisualDebugger => "visualDebugger",
            Command::ReviewPanel => "reviewPanel",
            Command::Workspace => "workspace",
            Command::LocalAi => "localAI",
            Command::NextHeading => "nextHeading",
            Command::PreviousHeading => "previousHeading",
            Command::NextChange => "nextChange",
            Command::PreviousChange => "previousChange",
            Command::MarkChangesReviewed => "markChangesReviewed",
            Command::FollowLinkAtCaret => "followLinkAtCaret",
            Command::NextLink => "nextLink",
            Command::PreviousLink => "previousLink",
            Command::ScrollDown => "scrollDown",
            Command::ScrollUp => "scrollUp",
            Command::PageDown => "pageDown",
            Command::PageUp => "pageUp",
            Command::DocumentStart => "documentStart",
            Command::DocumentEnd => "documentEnd",
            Command::GoBack => "goBack",
            Command::GoForward => "goForward",
            Command::ZoomLevel1 => "zoomLevel1",
            Command::ZoomLevel2 => "zoomLevel2",
            Command::ZoomLevel3 => "zoomLevel3",
            Command::ZoomLevel4 => "zoomLevel4",
            Command::ZoomLevel5 => "zoomLevel5",
            Command::ZoomIn => "zoomIn",
            Command::ZoomOut => "zoomOut",
            Command::Find => "find",
            Command::FindNext => "findNext",
            Command::FindPrevious => "findPrevious",
            Command::FindReplace => "findReplace",
            Command::FindInSiblings => "findInSiblings",
            Command::UseSelectionForFind => "useSelectionForFind",
            Command::PromoteHeading => "promoteHeading",
            Command::DemoteHeading => "demoteHeading",
            Command::HeadingLevel1 => "headingLevel1",
            Command::HeadingLevel2 => "headingLevel2",
            Command::HeadingLevel3 => "headingLevel3",
            Command::HeadingLevel4 => "headingLevel4",
            Command::HeadingLevel5 => "headingLevel5",
            Command::HeadingLevel6 => "headingLevel6",
            Command::HeadingToBody => "headingToBody",
            Command::MoveBlockUp => "moveBlockUp",
            Command::MoveBlockDown => "moveBlockDown",
            Command::FoldSection => "foldSection",
            Command::UnfoldSection => "unfoldSection",
            Command::FoldAll => "foldAll",
            Command::UnfoldAll => "unfoldAll",
            Command::ConvertToParagraph => "convertToParagraph",
            Command::ConvertToBulletList => "convertToBulletList",
            Command::ConvertToNumberedList => "convertToNumberedList",
            Command::ConvertToTaskList => "convertToTaskList",
            Command::ConvertToBlockquote => "convertToBlockquote",
            Command::SortListAlphabetically => "sortListAlphabetically",
            Command::SortListByState => "sortListByState",
            Command::InsertTableOfContents => "insertTableOfContents",
            Command::TidyDocument => "tidyDocument",
            Command::ToggleBold => "toggleBold",
            Command::ToggleItalic => "toggleItalic",
            Command::InsertLink => "insertLink",
            Command::ToggleStrikethrough => "toggleStrikethrough",
            Command::ToggleInlineCode => "toggleInlineCode",
            Command::IndentList => "indentList",
            Command::OutdentList => "outdentList",
            Command::ToggleTaskAtCaret => "toggleTaskAtCaret",
            Command::NewDocument => "newDocument",
            Command::Open => "open",
            Command::Save => "save",
            Command::SaveAs => "saveAs",
            Command::RevealInFinder => "revealInFinder",
            Command::OpenInEditor => "openInEditor",
            Command::Close => "close",
            Command::QuickLook => "quickLook",
            Command::CopyAsMarkdown => "copyAsMarkdown",
            Command::CopyAsRichText => "copyAsRichText",
            Command::CopyAsPlainText => "copyAsPlainText",
            Command::CopySection => "copySection",
            Command::CopySectionLink => "copySectionLink",
            Command::PrintDocument => "printDocument",
            Command::ExportHtml => "exportHTML",
            Command::ExportPdf => "exportPDF",
            Command::ExportSelectionAsImage => "exportSelectionAsImage",
            Command::Share => "share",
            Command::ShareAsPdf => "shareAsPDF",
            Command::IncreaseTextSize => "increaseTextSize",
            Command::DecreaseTextSize => "decreaseTextSize",
            Command::ResetTextSize => "resetTextSize",
            Command::SpeakDocument => "speakDocument",
            Command::StopSpeaking => "stopSpeaking",
            Command::Preferences => "preferences",
            Command::ReloadTheme => "reloadTheme",
            Command::ShowKeybindings => "showKeybindings",
            Command::CheckForUpdates => "checkForUpdates",
            Command::ToggleLightDark => "toggleLightDark",
            Command::GoToLine => "goToLine",
        }
    }

    /// `Command(rawValue:)`. Swift matches raw values with `==`, so a
    /// canonically equivalent spelling (`show\u{212A}eybindings`) matches too.
    pub fn from_raw_value(raw: &str) -> Option<Command> {
        let key = ascii_key(raw)?;
        Some(match key.as_ref() {
            "sourceMode" => Command::SourceMode,
            "splitView" => Command::SplitView,
            "pinWindow" => Command::PinWindow,
            "focusMode" => Command::FocusMode,
            "typewriterScrolling" => Command::TypewriterScrolling,
            "statusBar" => Command::StatusBar,
            "taskPanel" => Command::TaskPanel,
            "versionTimeline" => Command::VersionTimeline,
            "compareFiles" => Command::CompareFiles,
            "frontMatterEditor" => Command::FrontMatterEditor,
            "tableEditor" => Command::TableEditor,
            "assetDoctor" => Command::AssetDoctor,
            "commandPalette" => Command::CommandPalette,
            "documentLens" => Command::DocumentLens,
            "readerProfiles" => Command::ReaderProfiles,
            "documentHealth" => Command::DocumentHealth,
            "renderTargets" => Command::RenderTargets,
            "visualDebugger" => Command::VisualDebugger,
            "reviewPanel" => Command::ReviewPanel,
            "workspace" => Command::Workspace,
            "localAI" => Command::LocalAi,
            "nextHeading" => Command::NextHeading,
            "previousHeading" => Command::PreviousHeading,
            "nextChange" => Command::NextChange,
            "previousChange" => Command::PreviousChange,
            "markChangesReviewed" => Command::MarkChangesReviewed,
            "followLinkAtCaret" => Command::FollowLinkAtCaret,
            "nextLink" => Command::NextLink,
            "previousLink" => Command::PreviousLink,
            "scrollDown" => Command::ScrollDown,
            "scrollUp" => Command::ScrollUp,
            "pageDown" => Command::PageDown,
            "pageUp" => Command::PageUp,
            "documentStart" => Command::DocumentStart,
            "documentEnd" => Command::DocumentEnd,
            "goBack" => Command::GoBack,
            "goForward" => Command::GoForward,
            "zoomLevel1" => Command::ZoomLevel1,
            "zoomLevel2" => Command::ZoomLevel2,
            "zoomLevel3" => Command::ZoomLevel3,
            "zoomLevel4" => Command::ZoomLevel4,
            "zoomLevel5" => Command::ZoomLevel5,
            "zoomIn" => Command::ZoomIn,
            "zoomOut" => Command::ZoomOut,
            "find" => Command::Find,
            "findNext" => Command::FindNext,
            "findPrevious" => Command::FindPrevious,
            "findReplace" => Command::FindReplace,
            "findInSiblings" => Command::FindInSiblings,
            "useSelectionForFind" => Command::UseSelectionForFind,
            "promoteHeading" => Command::PromoteHeading,
            "demoteHeading" => Command::DemoteHeading,
            "headingLevel1" => Command::HeadingLevel1,
            "headingLevel2" => Command::HeadingLevel2,
            "headingLevel3" => Command::HeadingLevel3,
            "headingLevel4" => Command::HeadingLevel4,
            "headingLevel5" => Command::HeadingLevel5,
            "headingLevel6" => Command::HeadingLevel6,
            "headingToBody" => Command::HeadingToBody,
            "moveBlockUp" => Command::MoveBlockUp,
            "moveBlockDown" => Command::MoveBlockDown,
            "foldSection" => Command::FoldSection,
            "unfoldSection" => Command::UnfoldSection,
            "foldAll" => Command::FoldAll,
            "unfoldAll" => Command::UnfoldAll,
            "convertToParagraph" => Command::ConvertToParagraph,
            "convertToBulletList" => Command::ConvertToBulletList,
            "convertToNumberedList" => Command::ConvertToNumberedList,
            "convertToTaskList" => Command::ConvertToTaskList,
            "convertToBlockquote" => Command::ConvertToBlockquote,
            "sortListAlphabetically" => Command::SortListAlphabetically,
            "sortListByState" => Command::SortListByState,
            "insertTableOfContents" => Command::InsertTableOfContents,
            "tidyDocument" => Command::TidyDocument,
            "toggleBold" => Command::ToggleBold,
            "toggleItalic" => Command::ToggleItalic,
            "insertLink" => Command::InsertLink,
            "toggleStrikethrough" => Command::ToggleStrikethrough,
            "toggleInlineCode" => Command::ToggleInlineCode,
            "indentList" => Command::IndentList,
            "outdentList" => Command::OutdentList,
            "toggleTaskAtCaret" => Command::ToggleTaskAtCaret,
            "newDocument" => Command::NewDocument,
            "open" => Command::Open,
            "save" => Command::Save,
            "saveAs" => Command::SaveAs,
            "revealInFinder" => Command::RevealInFinder,
            "openInEditor" => Command::OpenInEditor,
            "close" => Command::Close,
            "quickLook" => Command::QuickLook,
            "copyAsMarkdown" => Command::CopyAsMarkdown,
            "copyAsRichText" => Command::CopyAsRichText,
            "copyAsPlainText" => Command::CopyAsPlainText,
            "copySection" => Command::CopySection,
            "copySectionLink" => Command::CopySectionLink,
            "printDocument" => Command::PrintDocument,
            "exportHTML" => Command::ExportHtml,
            "exportPDF" => Command::ExportPdf,
            "exportSelectionAsImage" => Command::ExportSelectionAsImage,
            "share" => Command::Share,
            "shareAsPDF" => Command::ShareAsPdf,
            "increaseTextSize" => Command::IncreaseTextSize,
            "decreaseTextSize" => Command::DecreaseTextSize,
            "resetTextSize" => Command::ResetTextSize,
            "speakDocument" => Command::SpeakDocument,
            "stopSpeaking" => Command::StopSpeaking,
            "preferences" => Command::Preferences,
            "reloadTheme" => Command::ReloadTheme,
            "showKeybindings" => Command::ShowKeybindings,
            "checkForUpdates" => Command::CheckForUpdates,
            "toggleLightDark" => Command::ToggleLightDark,
            "goToLine" => Command::GoToLine,
            _ => return None,
        })
    }

    pub fn title(self) -> &'static str {
        match self {
            Command::SourceMode => "Source Focus",
            Command::SplitView => "Split View",
            Command::PinWindow => "Pin Window",
            Command::FocusMode => "Focus Mode",
            Command::TypewriterScrolling => "Typewriter Scrolling",
            Command::StatusBar => "Status Bar",
            Command::TaskPanel => "Tasks",
            Command::VersionTimeline => "Version Timeline",
            Command::CompareFiles => "Compare Files…",
            Command::FrontMatterEditor => "Front Matter",
            Command::TableEditor => "Edit Table…",
            Command::AssetDoctor => "Asset Doctor",
            Command::CommandPalette => "Command Palette…",
            Command::DocumentLens => "Contents / Outline",
            Command::ReaderProfiles => "Reader Profiles",
            Command::DocumentHealth => "Document Health",
            Command::RenderTargets => "Render Targets",
            Command::VisualDebugger => "Visual Debugger",
            Command::ReviewPanel => "Review",
            Command::Workspace => "Workspace",
            Command::LocalAi => "On-Device AI",
            Command::NextHeading => "Next Heading",
            Command::PreviousHeading => "Previous Heading",
            Command::NextChange => "Next Change",
            Command::PreviousChange => "Previous Change",
            Command::MarkChangesReviewed => "Mark Changes Reviewed",
            Command::FollowLinkAtCaret => "Open Link at Caret",
            Command::NextLink => "Next Link",
            Command::PreviousLink => "Previous Link",
            Command::ScrollDown => "Scroll Down",
            Command::ScrollUp => "Scroll Up",
            Command::PageDown => "Page Down",
            Command::PageUp => "Page Up",
            Command::DocumentStart => "Top of Document",
            Command::DocumentEnd => "End of Document",
            Command::GoBack => "Back",
            Command::GoForward => "Forward",
            Command::ZoomLevel1 => "Detail: Top-Level Headings",
            Command::ZoomLevel2 => "Detail: Headings Through Level 2",
            Command::ZoomLevel3 => "Detail: All Headings",
            Command::ZoomLevel4 => "Detail: Outline and Summaries",
            Command::ZoomLevel5 => "Detail: Full Document",
            Command::ZoomIn => "Show More Detail",
            Command::ZoomOut => "Show Less Detail",
            Command::Find => "Find…",
            Command::FindNext => "Find Next",
            Command::FindPrevious => "Find Previous",
            Command::FindReplace => "Find and Replace…",
            Command::FindInSiblings => "Find in Sibling Files…",
            Command::UseSelectionForFind => "Use Selection for Find",
            Command::PromoteHeading => "Promote Heading",
            Command::DemoteHeading => "Demote Heading",
            Command::HeadingLevel1 => "Heading 1",
            Command::HeadingLevel2 => "Heading 2",
            Command::HeadingLevel3 => "Heading 3",
            Command::HeadingLevel4 => "Heading 4",
            Command::HeadingLevel5 => "Heading 5",
            Command::HeadingLevel6 => "Heading 6",
            Command::HeadingToBody => "Body Text",
            Command::MoveBlockUp => "Move Block Up",
            Command::MoveBlockDown => "Move Block Down",
            Command::FoldSection => "Fold Section",
            Command::UnfoldSection => "Unfold Section",
            Command::FoldAll => "Fold All",
            Command::UnfoldAll => "Unfold All",
            Command::ConvertToParagraph => "Convert to Paragraph",
            Command::ConvertToBulletList => "Convert to Bullet List",
            Command::ConvertToNumberedList => "Convert to Numbered List",
            Command::ConvertToTaskList => "Convert to Task List",
            Command::ConvertToBlockquote => "Convert to Blockquote",
            Command::SortListAlphabetically => "Sort List Alphabetically",
            Command::SortListByState => "Sort List by Checkbox",
            Command::InsertTableOfContents => "Insert Table of Contents",
            Command::TidyDocument => "Tidy Document…",
            Command::ToggleBold => "Bold",
            Command::ToggleItalic => "Italic",
            Command::InsertLink => "Link",
            Command::ToggleStrikethrough => "Strikethrough",
            Command::ToggleInlineCode => "Inline Code",
            Command::IndentList => "Indent",
            Command::OutdentList => "Outdent",
            Command::ToggleTaskAtCaret => "Toggle Task",
            Command::NewDocument => "New…",
            Command::Open => "Open…",
            Command::Save => "Save",
            Command::SaveAs => "Save As…",
            Command::RevealInFinder => "Reveal in Finder",
            Command::OpenInEditor => "Open in Editor",
            Command::Close => "Close",
            Command::QuickLook => "Quick Look",
            Command::CopyAsMarkdown => "Copy as Markdown",
            Command::CopyAsRichText => "Copy as Rich Text",
            Command::CopyAsPlainText => "Copy as Plain Text",
            Command::CopySection => "Copy Section",
            Command::CopySectionLink => "Copy Link to Section",
            Command::PrintDocument => "Print…",
            Command::ExportHtml => "Export HTML…",
            Command::ExportPdf => "Export PDF…",
            Command::ExportSelectionAsImage => "Export Selection as Image…",
            Command::Share => "Share…",
            Command::ShareAsPdf => "Share as PDF…",
            Command::IncreaseTextSize => "Bigger Text",
            Command::DecreaseTextSize => "Smaller Text",
            Command::ResetTextSize => "Actual Size",
            Command::SpeakDocument => "Speak Selection or Document",
            Command::StopSpeaking => "Stop Speaking",
            Command::Preferences => "Settings…",
            Command::ReloadTheme => "Reload Themes",
            Command::ShowKeybindings => "Keyboard Shortcuts…",
            Command::CheckForUpdates => "Check for Updates…",
            Command::ToggleLightDark => "Toggle Light/Dark Theme",
            Command::GoToLine => "Go to Line…",
        }
    }

    /// Where the command appears in the menu bar.
    ///
    /// `MainMenu` builds every menu from this, and the keybinding editor shows
    /// it in the "Menu" column, so a command placed here is a promise about
    /// where the user will find it. `CommandTableTests` holds us to it.
    pub fn menu(self) -> Menu {
        use Command::*;
        match self {
            NewDocument | Open | Save | SaveAs | Close | RevealInFinder | OpenInEditor | PrintDocument | ExportHtml
            | ExportPdf | ExportSelectionAsImage | CompareFiles | VersionTimeline | Share | ShareAsPdf | QuickLook => {
                Menu::File
            }
            CopyAsMarkdown | CopyAsRichText | CopyAsPlainText | CopySection | CopySectionLink | Find | FindNext
            | FindPrevious | FindReplace | FindInSiblings | UseSelectionForFind | SpeakDocument | StopSpeaking => {
                Menu::Edit
            }
            ToggleBold | ToggleItalic | InsertLink | ToggleStrikethrough | ToggleInlineCode | ConvertToParagraph
            | ConvertToBulletList | ConvertToNumberedList | ConvertToTaskList | ConvertToBlockquote | IndentList
            | OutdentList | ToggleTaskAtCaret | PromoteHeading | DemoteHeading | HeadingLevel1 | HeadingLevel2
            | HeadingLevel3 | HeadingLevel4 | HeadingLevel5 | HeadingLevel6 | HeadingToBody => Menu::Format,
            SourceMode | ZoomLevel1 | ZoomLevel2 | ZoomLevel3 | ZoomLevel4 | ZoomLevel5 | ZoomIn | ZoomOut
            | IncreaseTextSize | DecreaseTextSize | ResetTextSize | FocusMode | TypewriterScrolling | StatusBar
            | TaskPanel | ReloadTheme | CommandPalette | DocumentLens | ReaderProfiles | DocumentHealth
            | RenderTargets | VisualDebugger | ReviewPanel | Workspace | LocalAi => Menu::View,
            NextHeading | PreviousHeading | NextChange | PreviousChange | MarkChangesReviewed | FollowLinkAtCaret
            | NextLink | PreviousLink | ScrollDown | ScrollUp | PageDown | PageUp | DocumentStart | DocumentEnd
            | GoBack | GoForward => Menu::Navigate,
            MoveBlockUp | MoveBlockDown | FoldSection | UnfoldSection | FoldAll | UnfoldAll | SortListAlphabetically
            | SortListByState | InsertTableOfContents | TidyDocument | FrontMatterEditor | TableEditor
            | AssetDoctor => Menu::Document,
            SplitView | PinWindow => Menu::Window,
            // Settings…, Keyboard Shortcuts…, and Check for Updates… live in
            // the app menu on macOS, and that is what the editor must say.
            // Toggle Light/Dark is here too — it's an app-wide toggle, not a
            // per-document setting.
            Preferences | ShowKeybindings | CheckForUpdates | ToggleLightDark => Menu::Application,
            GoToLine => Menu::Navigate,
        }
    }

    /// Modes in which the command is dispatchable, in `CommandScope.allCases`
    /// order (Swift returns a `Set`; every caller filters `allCases` by it or
    /// tests membership).
    pub fn scopes(self) -> &'static [CommandScope] {
        use Command::*;
        match self {
            ToggleBold | ToggleItalic | InsertLink | ToggleStrikethrough | ToggleInlineCode | IndentList
            | OutdentList => &[CommandScope::Live],
            _ => &[CommandScope::Read, CommandScope::Live, CommandScope::Source],
        }
    }

    /// What must hold before this command can do its job. Menu validation is
    /// derived from this, so an item is never enabled in a state where running
    /// it would be a no-op.
    pub fn requires(self) -> CommandPrecondition {
        use Command::*;
        match self {
            NewDocument | Open | Preferences | ShowKeybindings | ReloadTheme | CompareFiles | ToggleLightDark => {
                CommandPrecondition::Always
            }
            CheckForUpdates => CommandPrecondition::UpdateCheck,
            Save => CommandPrecondition::UnsavedChanges,
            FindNext | FindPrevious => CommandPrecondition::FindQuery,
            GoBack => CommandPrecondition::BackHistory,
            GoForward => CommandPrecondition::ForwardHistory,
            StopSpeaking => CommandPrecondition::Speaking,
            TableEditor => CommandPrecondition::TableAtCaret,
            RevealInFinder | OpenInEditor | VersionTimeline | CopySectionLink | FindInSiblings => {
                CommandPrecondition::DocumentWithFile
            }
            UseSelectionForFind | ExportSelectionAsImage => CommandPrecondition::Selection,
            // Not `.selection`: the caret sitting *on* a link is enough, and
            // requiring a highlighted range would disable the command in the
            // state it is most often wanted from.
            QuickLook => CommandPrecondition::QuickLookTarget,
            // Deliberately `.document`, not `.documentWithFile`: a never-saved
            // buffer still holds something worth sending.
            Share | ShareAsPdf => CommandPrecondition::Document,
            NextChange | PreviousChange | MarkChangesReviewed => CommandPrecondition::ChangeMarks,
            _ => CommandPrecondition::Document,
        }
    }

    pub fn is_enabled(self, context: &CommandContext) -> bool {
        self.requires().is_satisfied(context)
    }
}

/// `Command.Menu`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Menu {
    Application,
    File,
    Edit,
    Format,
    View,
    Navigate,
    Document,
    Window,
    Help,
}

impl Menu {
    pub const ALL_CASES: [Menu; 9] = [
        Menu::Application,
        Menu::File,
        Menu::Edit,
        Menu::Format,
        Menu::View,
        Menu::Navigate,
        Menu::Document,
        Menu::Window,
        Menu::Help,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Menu::Application => "application",
            Menu::File => "file",
            Menu::Edit => "edit",
            Menu::Format => "format",
            Menu::View => "view",
            Menu::Navigate => "navigate",
            Menu::Document => "document",
            Menu::Window => "window",
            Menu::Help => "help",
        }
    }

    /// The app menu is named after the app, not after the enum case;
    /// "Application" would be a lie in both the menu bar and Settings.
    pub fn title(self) -> String {
        if self == Menu::Application { "Upleft".to_owned() } else { swift_text::capitalized(self.raw_value()) }
    }
}

/// `enum CommandScope: String, Codable, CaseIterable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CommandScope {
    Read,
    Live,
    Source,
}

impl CommandScope {
    pub const ALL_CASES: [CommandScope; 3] = [CommandScope::Read, CommandScope::Live, CommandScope::Source];

    pub fn raw_value(self) -> &'static str {
        match self {
            CommandScope::Read => "read",
            CommandScope::Live => "live",
            CommandScope::Source => "source",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<CommandScope> {
        CommandScope::ALL_CASES.into_iter().find(|scope| swift_eq(raw, scope.raw_value()))
    }
}

// MARK: - Preconditions

/// The one condition a command needs before it can run. A typed value rather
/// than a hand-written `validateMenuItem` switch, so the menu bar, the palette,
/// and any future toolbar all agree by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandPrecondition {
    /// Runs with no document open — the start-window state.
    Always,
    /// Needs a document window.
    Document,
    /// Needs a document that exists on disk (paths, siblings, history).
    DocumentWithFile,
    /// Needs a non-empty selection.
    Selection,
    /// Needs a configured, idle updater.
    UpdateCheck,
    UnsavedChanges,
    FindQuery,
    BackHistory,
    ForwardHistory,
    Speaking,
    TableAtCaret,
    /// Needs at least one live change mark to walk or to retire.
    ChangeMarks,
    /// Needs the caret to be on something with a file behind it.
    QuickLookTarget,
}

impl CommandPrecondition {
    pub const ALL_CASES: [CommandPrecondition; 13] = [
        CommandPrecondition::Always,
        CommandPrecondition::Document,
        CommandPrecondition::DocumentWithFile,
        CommandPrecondition::Selection,
        CommandPrecondition::UpdateCheck,
        CommandPrecondition::UnsavedChanges,
        CommandPrecondition::FindQuery,
        CommandPrecondition::BackHistory,
        CommandPrecondition::ForwardHistory,
        CommandPrecondition::Speaking,
        CommandPrecondition::TableAtCaret,
        CommandPrecondition::ChangeMarks,
        CommandPrecondition::QuickLookTarget,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            CommandPrecondition::Always => "always",
            CommandPrecondition::Document => "document",
            CommandPrecondition::DocumentWithFile => "documentWithFile",
            CommandPrecondition::Selection => "selection",
            CommandPrecondition::UpdateCheck => "updateCheck",
            CommandPrecondition::UnsavedChanges => "unsavedChanges",
            CommandPrecondition::FindQuery => "findQuery",
            CommandPrecondition::BackHistory => "backHistory",
            CommandPrecondition::ForwardHistory => "forwardHistory",
            CommandPrecondition::Speaking => "speaking",
            CommandPrecondition::TableAtCaret => "tableAtCaret",
            CommandPrecondition::ChangeMarks => "changeMarks",
            CommandPrecondition::QuickLookTarget => "quickLookTarget",
        }
    }

    pub fn is_satisfied(self, context: &CommandContext) -> bool {
        match self {
            CommandPrecondition::Always => true,
            CommandPrecondition::Document => context.has_document,
            CommandPrecondition::ChangeMarks => context.has_document && context.has_change_marks,
            CommandPrecondition::DocumentWithFile => context.has_document && context.document_has_file,
            CommandPrecondition::Selection => context.has_document && context.has_selection,
            CommandPrecondition::UpdateCheck => context.can_check_for_updates,
            CommandPrecondition::UnsavedChanges => {
                context.has_document && (context.has_unsaved_changes || !context.document_has_file)
            }
            CommandPrecondition::FindQuery => context.has_document && context.has_find_query,
            CommandPrecondition::BackHistory => context.has_document && context.can_go_back,
            CommandPrecondition::ForwardHistory => context.has_document && context.can_go_forward,
            CommandPrecondition::Speaking => context.has_document && context.is_speaking,
            CommandPrecondition::TableAtCaret => context.has_document && context.caret_is_in_table,
            CommandPrecondition::QuickLookTarget => context.has_document && context.has_quick_look_target,
        }
    }
}

/// The facts a `CommandPrecondition` is evaluated against. Callers assemble
/// one from whatever they own; the policy above stays pure and testable.
/// `CommandContext()` is [`CommandContext::default`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandContext {
    pub has_document: bool,
    pub document_has_file: bool,
    pub has_selection: bool,
    pub can_check_for_updates: bool,
    pub has_unsaved_changes: bool,
    pub has_find_query: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub is_speaking: bool,
    pub caret_is_in_table: bool,
    /// At least one unexpired change mark is on the page.
    pub has_change_marks: bool,
    /// The caret or selection is on a link, path token, or image.
    pub has_quick_look_target: bool,
}

impl CommandContext {
    /// No document anywhere — what the app menu sees while only the start
    /// window is up.
    pub fn application_only(can_check_for_updates: bool) -> CommandContext {
        CommandContext { can_check_for_updates, ..CommandContext::default() }
    }
}

// MARK: - Key bindings

/// `NSEvent.ModifierFlags`, with AppKit's raw values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModifierFlags(pub u64);

impl ModifierFlags {
    pub const EMPTY: ModifierFlags = ModifierFlags(0);
    pub const CAPS_LOCK: ModifierFlags = ModifierFlags(1 << 16);
    pub const SHIFT: ModifierFlags = ModifierFlags(1 << 17);
    pub const CONTROL: ModifierFlags = ModifierFlags(1 << 18);
    pub const OPTION: ModifierFlags = ModifierFlags(1 << 19);
    pub const COMMAND: ModifierFlags = ModifierFlags(1 << 20);
    pub const NUMERIC_PAD: ModifierFlags = ModifierFlags(1 << 21);
    pub const HELP: ModifierFlags = ModifierFlags(1 << 22);
    pub const FUNCTION: ModifierFlags = ModifierFlags(1 << 23);
    pub const DEVICE_INDEPENDENT_FLAGS_MASK: ModifierFlags = ModifierFlags(0xffff_0000);

    pub const fn raw_value(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: ModifierFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: ModifierFlags) -> ModifierFlags {
        ModifierFlags(self.0 | other.0)
    }

    pub const fn intersection(self, other: ModifierFlags) -> ModifierFlags {
        ModifierFlags(self.0 & other.0)
    }

    pub fn insert(&mut self, other: ModifierFlags) {
        self.0 |= other.0;
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for ModifierFlags {
    type Output = ModifierFlags;
    fn bitor(self, rhs: ModifierFlags) -> ModifierFlags {
        self.union(rhs)
    }
}

/// The modifier names used on disk. A typed value, so an unknown name is a
/// decode error the user is told about rather than a silently dropped
/// modifier that changes what their shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifier {
    Control,
    Option,
    Shift,
    Command,
    NumericPad,
}

impl Modifier {
    pub const ALL_CASES: [Modifier; 5] =
        [Modifier::Control, Modifier::Option, Modifier::Shift, Modifier::Command, Modifier::NumericPad];

    pub fn raw_value(self) -> &'static str {
        match self {
            Modifier::Control => "control",
            Modifier::Option => "option",
            Modifier::Shift => "shift",
            Modifier::Command => "command",
            Modifier::NumericPad => "numericPad",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Modifier> {
        Modifier::ALL_CASES.into_iter().find(|modifier| swift_eq(raw, modifier.raw_value()))
    }

    pub fn flag(self) -> ModifierFlags {
        match self {
            Modifier::Control => ModifierFlags::CONTROL,
            Modifier::Option => ModifierFlags::OPTION,
            Modifier::Shift => ModifierFlags::SHIFT,
            Modifier::Command => ModifierFlags::COMMAND,
            Modifier::NumericPad => ModifierFlags::NUMERIC_PAD,
        }
    }

    /// Stable order, so the file does not churn between writes.
    pub fn names(flags: ModifierFlags) -> Vec<Modifier> {
        Modifier::ALL_CASES.into_iter().filter(|modifier| flags.contains(modifier.flag())).collect()
    }

    pub fn flags(names: &[Modifier]) -> ModifierFlags {
        names.iter().fold(ModifierFlags::EMPTY, |flags, name| flags.union(name.flag()))
    }

    /// `Modifier(from:)`: a `String` raw value, anything else an error.
    pub fn decode(value: &Value) -> Result<Modifier, DecodingError> {
        let raw = json_decoder::decode_string(value)?;
        Modifier::from_raw_value(&raw).ok_or_else(|| {
            DecodingError::DataCorrupted(format!("Cannot initialize Modifier from invalid String value {raw}"))
        })
    }
}

/// A single chord.
///
/// Serialised as `{"key": "+", "modifiers": ["command"]}`. The old `"cmd++"`
/// string form is still *read* so existing files keep working, but it is
/// never written: it could not represent `+` without ambiguity, and a binding
/// it failed to parse took the whole keybindings file down with it.
#[derive(Clone, Debug)]
pub struct KeyBinding {
    /// `"e"`, `"space"`, `"left"`, `"["`, `"+"`.
    pub key: String,
    pub modifiers: ModifierFlags,
}

impl KeyBinding {
    /// `KeyBinding(_:_:)`.
    pub fn new(key: impl Into<String>, modifiers: ModifierFlags) -> KeyBinding {
        KeyBinding { key: key.into(), modifiers: KeyBinding::normalized(modifiers) }
    }

    /// Modifier bits that actually mean something to a shortcut. Caps Lock and
    /// Fn are sticky state, not a chord, and an enabled Caps Lock must not make
    /// `⌘S` stop matching. Applied on construction and on every equality/hash
    /// so a stored binding and an incoming event always agree.
    pub fn normalized(flags: ModifierFlags) -> ModifierFlags {
        flags.intersection(
            ModifierFlags::COMMAND
                | ModifierFlags::SHIFT
                | ModifierFlags::OPTION
                | ModifierFlags::CONTROL
                | ModifierFlags::NUMERIC_PAD,
        )
    }

    // MARK: Serialisation

    /// `KeyBinding(parsing:)`: the legacy string form, kept for reading old
    /// files and for diagnostics.
    ///
    /// `"cmd++"` (⌘ plus the `+` key) is the case that used to be unreadable:
    /// splitting on `+` and dropping empty parts left no key at all.
    pub fn parsing(string: &str) -> Option<KeyBinding> {
        let lowered = swift_text::lowercased(string);
        let mut parts: Vec<String> =
            swift_text::split(&lowered, '+', usize::MAX, false).into_iter().map(str::to_owned).collect();
        // "+" → ["", ""] and "cmd++" → ["cmd", "", ""]: two empty tails are the
        // literal `+` key, not two empty components.
        if parts.len() >= 2 && parts[parts.len() - 1].is_empty() && parts[parts.len() - 2].is_empty() {
            parts.truncate(parts.len() - 2);
            parts.push("+".to_owned());
        }
        let key = parts.pop().filter(|key| !key.is_empty())?;
        let mut flags = ModifierFlags::EMPTY;
        for part in &parts {
            let name = ascii_key(part)?;
            match name.as_ref() {
                "cmd" | "command" => flags.insert(ModifierFlags::COMMAND),
                "shift" => flags.insert(ModifierFlags::SHIFT),
                "opt" | "option" | "alt" => flags.insert(ModifierFlags::OPTION),
                "ctrl" | "control" => flags.insert(ModifierFlags::CONTROL),
                _ => return None,
            }
        }
        Some(KeyBinding::new(key, flags))
    }

    /// `serialized`.
    pub fn serialized(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.modifiers.contains(ModifierFlags::CONTROL) {
            parts.push("ctrl");
        }
        if self.modifiers.contains(ModifierFlags::OPTION) {
            parts.push("opt");
        }
        if self.modifiers.contains(ModifierFlags::SHIFT) {
            parts.push("shift");
        }
        if self.modifiers.contains(ModifierFlags::COMMAND) {
            parts.push("cmd");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// `init(from:)`: the legacy string form, or `{"key", "modifiers"}`.
    pub fn decode(value: &Value) -> Result<KeyBinding, DecodingError> {
        if let Ok(raw) = json_decoder::decode_string(value) {
            return KeyBinding::parsing(&raw)
                .ok_or_else(|| DecodingError::DataCorrupted(format!("bad key binding \"{raw}\"")));
        }
        let members = json_decoder::keyed(value)?;
        let key = json_decoder::decode_string(json_decoder::member(members, "key")?)?;
        if key.is_empty() {
            return Err(DecodingError::DataCorrupted("key binding has an empty key".into()));
        }
        let modifiers = match json_decoder::member_if_present(members, "modifiers") {
            Some(value) => json_decoder::unkeyed(value)?.iter().map(Modifier::decode).collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        Ok(KeyBinding::new(key, Modifier::flags(&modifiers)))
    }

    /// `encode(to:)`: members in `CodingKeys` order.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("key", JsonValue::String(self.key.clone())),
            (
                "modifiers",
                JsonValue::Array(
                    Modifier::names(self.modifiers).into_iter().map(|name| JsonValue::from(name.raw_value())).collect(),
                ),
            ),
        ])
    }

    // MARK: Display

    /// `⌘⇧O`, `⌥↓`, `Space`.
    pub fn display_string(&self) -> String {
        let mut out = String::new();
        if self.modifiers.contains(ModifierFlags::CONTROL) {
            out.push('⌃');
        }
        if self.modifiers.contains(ModifierFlags::OPTION) {
            out.push('⌥');
        }
        if self.modifiers.contains(ModifierFlags::SHIFT) {
            out.push('⇧');
        }
        if self.modifiers.contains(ModifierFlags::COMMAND) {
            out.push('⌘');
        }
        out.push_str(&KeyBinding::display_key(&self.key));
        out
    }

    pub fn display_key(key: &str) -> String {
        if let Some(name) = ascii_key(key) {
            match name.as_ref() {
                "space" => return "Space".into(),
                "left" => return "←".into(),
                "right" => return "→".into(),
                "up" => return "↑".into(),
                "down" => return "↓".into(),
                "return" | "enter" => return "↩".into(),
                "tab" => return "⇥".into(),
                "escape" | "esc" => return "⎋".into(),
                "delete" | "backspace" => return "⌫".into(),
                "backslash" => return "\\".into(),
                _ => {}
            }
        }
        if swift_text::count(key) == 1 { swift_text::uppercased(key) } else { swift_text::capitalized(key) }
    }

    /// The `keyEquivalent` string AppKit wants for a menu item.
    pub fn menu_key_equivalent(&self) -> String {
        if let Some(name) = ascii_key(&self.key) {
            match name.as_ref() {
                "space" => return " ".into(),
                "left" => return "\u{F702}".into(),
                "right" => return "\u{F703}".into(),
                "up" => return "\u{F700}".into(),
                "down" => return "\u{F701}".into(),
                "return" | "enter" => return "\r".into(),
                "tab" => return "\t".into(),
                "backslash" => return "\\".into(),
                _ => {}
            }
        }
        self.key.clone()
    }

    /// `KeyBinding.key(for:)`: the normalised key name for an incoming event,
    /// so lookup is a dictionary hit.
    pub fn key_for_event(event: &NSEvent) -> Option<String> {
        let key_code = event.keyCode();
        let characters = || event.charactersIgnoringModifiers().map(|characters| characters.to_string());
        KeyBinding::key_for(key_code, characters)
    }

    /// [`key_for_event`](Self::key_for_event) on an event's `keyCode` and
    /// `charactersIgnoringModifiers` (read only when the key code is not a
    /// named key, as Swift reads it).
    pub fn key_for(key_code: u16, characters_ignoring_modifiers: impl FnOnce() -> Option<String>) -> Option<String> {
        match key_code {
            49 => Some("space".into()),
            123 => Some("left".into()),
            124 => Some("right".into()),
            125 => Some("down".into()),
            126 => Some("up".into()),
            36 | 76 => Some("return".into()),
            48 => Some("tab".into()),
            53 => Some("escape".into()),
            51 => Some("delete".into()),
            116 => Some("pageup".into()),
            121 => Some("pagedown".into()),
            _ => {
                let characters = characters_ignoring_modifiers().filter(|characters| !characters.is_empty())?;
                Some(swift_text::lowercased(&characters))
            }
        }
    }
}

impl PartialEq for KeyBinding {
    fn eq(&self, other: &KeyBinding) -> bool {
        swift_text::str_eq(&self.key, &other.key)
            && KeyBinding::normalized(self.modifiers) == KeyBinding::normalized(other.modifiers)
    }
}

impl Eq for KeyBinding {}

impl std::hash::Hash for KeyBinding {
    /// Swift hashes a `String` by its NFC, consistent with `==`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        swift_text::string_key(&self.key).hash(state);
        KeyBinding::normalized(self.modifiers).raw_value().hash(state);
    }
}
