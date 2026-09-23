import AppKit
import MarkdownCore
import MarkdownRender
// The popover's initialiser, panel, surface and content are `private`; the
// package builds DownrightApp with `-enable-private-imports`.
@_private(sourceFile: "UpdateNotesPopover.swift") import DownrightApp

/// `UpdateNotesPopover` scenes: the hover panel for one update.
///
/// State: `metadata` (see `UpdateScenes.metadata`, with
/// `itemDescriptionFeed` naming a feed under `corpus/updater/feeds/`) and
/// `isReady`. `present(from:)` refuses an anchor whose window is on no
/// screen, which the off-screen harness host always is, so the scene calls
/// the private initialiser with an anchor in a borderless host window
/// created at (-30000, -30000) and never ordered in. The captured window is
/// the popover's own borderless `NSPanel`; `afterShow` runs what `show()`
/// does to the surface (`refreshGlassAfterWindowAttach`, `present`) but
/// neither attaches it as a child window nor starts the pointer poll, whose
/// first tick would dismiss the panel in an app that is never active.
///
/// The surface takes the harness's style sheet (Reduce Motion forced on),
/// so `present()` snaps the pour to fully revealed with no spring.
///
/// The model also carries the release-notes reduction:
/// `UpdateNotesSummary.summary(from:)` of the description and every
/// `droppingLastLine` step after it.
@MainActor
final class UpdateNotesPopoverScene: PanelScene {
    private var popover: UpdateNotesPopover?
    private var host: NSWindow?
    private var metadata: UpdateMetadata?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let metadata = UpdateScenes.metadata(scenario.object("metadata"))
        self.metadata = metadata
        let host = NSWindow(
            contentRect: NSRect(x: -30000, y: -30000, width: 480, height: 44),
            styleMask: [.borderless], backing: .buffered, defer: false
        )
        host.isReleasedWhenClosed = false
        self.host = host
        let anchor = NSView(frame: NSRect(x: 340, y: 9, width: 128, height: 26))
        host.contentView?.addSubview(anchor)
        let popover = UpdateNotesPopover(
            anchor: anchor, window: host, metadata: metadata, isReady: scenario.bool("isReady"), sheet: styleSheet
        )
        self.popover = popover
        return popover.panel.contentView!
    }

    func ownWindow(_ panel: NSView) -> NSWindow? { popover?.panel }

    func afterShow(window: NSWindow, scenario: PanelScenario) {
        popover?.surface.refreshGlassAfterWindowAttach()
        popover?.surface.present()
    }

    func model() -> JSON {
        guard let popover, let metadata else { return .null }
        let summary = UpdateNotesSummary.summary(from: metadata.itemDescription)
        var drops: [JSON] = []
        var text = summary
        while drops.count < 20, let shorter = UpdateNotesSummary.droppingLastLine(text) {
            drops.append(.string(shorter))
            text = shorter
        }
        let content = popover.surface.glass.contentView.subviews.first as? UpdateNotesContentView
        let document = content?.notesScroll.documentView
        let notesText: JSON
        if let textView = document as? NSTextView {
            notesText = .string(textView.string)
        } else if let field = document as? NSTextField {
            notesText = .string(field.stringValue)
        } else {
            notesText = .null
        }
        return .object([
            ("summary", .string(summary)),
            ("drops", .array(drops)),
            ("windowFrame", PanelTree.rect(popover.panel.frame)),
            ("surfaceFrame", PanelTree.rect(popover.surface.frame)),
            ("bodyWindowRect", PanelTree.rect(popover.bodyWindowRect)),
            ("notesHeight", content.map { .double(Double($0.notesHeight.constant)) } ?? .null),
            ("notesDocumentFrame", document.map { PanelTree.rect($0.frame) } ?? .null),
            ("notesText", notesText),
            ("revealValue", .double(Double(popover.surface.reveal.value))),
            ("surfaceAlpha", .double(Double(popover.surface.alphaValue))),
            ("reducesMotion", .bool(popover.surface.reducesMotion)),
            ("panelLabel", .string(popover.panel.accessibilityLabel())),
            ("panelRole", .string(popover.panel.accessibilityRole()?.rawValue)),
        ])
    }
}
