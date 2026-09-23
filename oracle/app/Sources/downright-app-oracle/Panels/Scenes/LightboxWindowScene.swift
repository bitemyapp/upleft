import AppKit
// `lightboxView`, `LightboxContentView`, `scale`, `offset`, `fitScale` and
// `imageRect` are `private` to LightboxWindow.swift; DownrightApp is built
// with `-enable-private-imports` (see LocalAIDump.swift).
@_private(sourceFile: "LightboxWindow.swift") import DownrightApp
import MarkdownCore
import MarkdownRender

/// `LightboxWindow` scenes. The window is the scene's own (`ownWindow`): a
/// `LightboxWindow` built from `state.image` (repository-relative; default
/// `corpus/render-images/img/photo.jpg`), `state.caption` and
/// `state.reduceTransparency`, with `reduceMotion` from the style sheet (the
/// harness forces it on, so every zoom is immediate and the scene settles).
///
/// `present(over:)` is applied without the parts that reach a screen: the
/// frame is the scenario's size at (-30000, -30000) instead of the parent's
/// screen, there is no parent window and no `makeKeyAndOrderFront`; the
/// zoom is reset and the alpha set as `present` does, and the first
/// responder is set after the harness orders the window in. Then each of
/// `state.keys` (`{characters, keyCode}`) is sent to the content view's
/// `keyDown(with:)` as a key-down event.
@MainActor
final class LightboxWindowScene: PanelScene {
    private var window: LightboxWindow?

    static func keyEvent(_ value: Any) -> NSEvent? {
        guard let object = value as? [String: Any] else { return nil }
        let characters = object["characters"] as? String ?? ""
        let keyCode = UInt16((object["keyCode"] as? NSNumber)?.intValue ?? 0)
        return NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0,
            context: nil, characters: characters, charactersIgnoringModifiers: characters,
            isARepeat: false, keyCode: keyCode
        )
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let path = repositoryRoot.appendingPathComponent(
            scenario.string("image", "corpus/render-images/img/photo.jpg")
        ).path
        guard let image = NSImage(contentsOfFile: path) else {
            throw PanelHarnessError.usage("no image at \(path)")
        }
        let window = LightboxWindow(
            image: image,
            caption: scenario.string("caption"),
            reduceMotion: styleSheet.reduceMotion,
            reduceTransparency: scenario.bool("reduceTransparency")
        )
        window.setFrame(
            NSRect(x: -30000, y: -30000, width: scenario.width, height: scenario.height),
            display: true
        )
        window.lightboxView?.resetZoom(animated: false)
        window.alphaValue = styleSheet.reduceMotion ? 1 : 0
        for value in scenario.array("keys") {
            guard let event = Self.keyEvent(value) else { continue }
            window.contentView?.keyDown(with: event)
        }
        self.window = window
        return window.contentView!
    }

    func ownWindow(_ panel: NSView) -> NSWindow? { window }

    func afterShow(window: NSWindow, scenario: PanelScenario) {
        window.makeFirstResponder(window.contentView)
    }

    func model() -> JSON {
        guard let window, let view = window.lightboxView else { return .null }
        return .object([
            ("scale", .double(Double(view.scale))),
            ("fitScale", .double(Double(view.fitScale))),
            ("offset", PanelTree.size(view.offset)),
            ("imageRect", PanelTree.rect(view.imageRect)),
            ("alpha", .double(Double(window.alphaValue))),
            ("canBecomeKey", .bool(window.canBecomeKey)),
            ("acceptsFirstResponder", .bool(view.acceptsFirstResponder)),
            ("collectionBehavior", .int(Int(window.collectionBehavior.rawValue))),
            ("animationBehavior", .int(window.animationBehavior.rawValue)),
            ("releasedWhenClosed", .bool(window.isReleasedWhenClosed)),
        ])
    }
}
