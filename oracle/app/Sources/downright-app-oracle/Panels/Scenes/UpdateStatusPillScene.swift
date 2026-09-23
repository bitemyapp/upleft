import AppKit
import MarkdownCore
import MarkdownRender
// The pill's subviews and `currentModel` are `private`; the package builds
// DownrightApp with `-enable-private-imports` (see LocalAIDump.swift).
@_private(sourceFile: "UpdateStatusPill.swift") import DownrightApp

/// `UpdateStatusPill` scenes: the titlebar/start-window pill in each state
/// of `UpdateCoordinator.shared` (the only coordinator a pill reads).
///
/// State: `presentation` (`"standard"` or `"compactWarning"`), `steps` (the
/// driver callbacks that move the shared coordinator, see `UpdateScenes`),
/// and `buildFirst` (build the pill before the steps, so it follows them
/// through `stateDidChange`, as a pill in an open window does). The pill is
/// hosted in a plain themed container, trailing-aligned like the titlebar
/// cluster.
///
/// The pill draws from its own style sheet — the current theme against
/// `NSApp`'s appearance, with the *system* Reduce Motion — not the
/// harness's; the scene selects the scenario's theme in `ThemeStore.shared`
/// for this process (`UpdateScenes.selectTheme`). With Reduce Motion off
/// the pill animates its width and shell alpha through `animator()` and the
/// harness waits for them to settle; with it on, it snaps. The arrival
/// emphasis never plays: the harness never activates the app.
@MainActor
final class UpdateStatusPillScene: PanelScene {
    private var pill: UpdateStatusPill?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        UpdateScenes.selectTheme(scenario.theme)
        let coordinator = UpdateCoordinator.shared
        coordinator.suppressUIForTesting = true
        coordinator.tearDownForTesting()
        let presentation: UpdateStatusPill.Presentation =
            scenario.string("presentation") == "compactWarning" ? .compactWarning : .standard
        var early: UpdateStatusPill?
        if scenario.bool("buildFirst") { early = UpdateStatusPill(presentation: presentation) }
        UpdateScenes.run(scenario.array("steps"), on: coordinator, engine: nil)
        let pill = early ?? UpdateStatusPill(presentation: presentation)
        self.pill = pill

        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        container.wantsLayer = true
        container.layer?.backgroundColor = styleSheet.background.cgColor
        pill.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(pill)
        NSLayoutConstraint.activate([
            pill.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -12),
            pill.centerYAnchor.constraint(equalTo: container.centerYAnchor),
        ])
        return container
    }

    func model() -> JSON {
        guard let pill else { return .null }
        let label = pill.label
        let progress = pill.progressIndicator
        return .object([
            ("pill", UpdateScenes.json(UpdateCoordinator.shared.pillModel)),
            ("currentModel", UpdateScenes.json(pill.currentModel)),
            ("title", .string(pill.title)),
            ("intrinsicContentSize", PanelTree.size(pill.intrinsicContentSize)),
            ("hidden", .bool(pill.isHidden)),
            ("labelText", .string(label.stringValue)),
            ("labelHidden", .bool(label.isHidden)),
            ("iconHidden", .bool(pill.iconView.isHidden)),
            ("iconHasImage", .bool(pill.iconView.image != nil)),
            ("progressHidden", .bool(progress.isHidden)),
            ("progressStyle", .int(Int(progress.style.rawValue))),
            ("progressIndeterminate", .bool(progress.isIndeterminate)),
            ("progressValue", .double(progress.doubleValue)),
            ("pendingUpdate", .string(UpdateCoordinator.shared.pendingUpdate?.displayVersionString)),
        ])
    }
}

/// Shared by the three update scenes (`UpdateStatusPill`,
/// `UpdateNotesPopover`, `UpdateWindowController`); mirrored by
/// `crates/conformance/src/dump/panel/scenes/update_status_pill.rs`.
@MainActor
enum UpdateScenes {
    /// Selects `name` in `ThemeStore.shared` for this process only. The
    /// update surfaces draw from the current theme, not the harness's style
    /// sheet; the selection is removed from the defaults again so it never
    /// reaches another scene through the shared conform home.
    static func selectTheme(_ name: String) {
        ThemeStore.shared.select(named: name)
        UserDefaults.standard.removeObject(forKey: "downright.theme.selected")
    }

    /// The text between the first `<![CDATA[` and the next `]]>` of a feed
    /// under `corpus/updater/feeds/` (each feed's item description).
    static func feedDescription(_ path: String) -> String? {
        let url = repositoryRoot.appendingPathComponent(path)
        guard let text = try? String(contentsOf: url, encoding: .utf8),
              let start = text.range(of: "<![CDATA["),
              let end = text.range(of: "]]>", range: start.upperBound..<text.endIndex)
        else { return nil }
        return String(text[start.upperBound..<end.lowerBound])
    }

    /// `{"versionString", "displayVersionString", "title", "itemDescription"
    /// | "itemDescriptionFeed", "releaseNotesURL", "infoURL",
    /// "contentLength", "isInformationOnly", "isCritical"}`.
    static func metadata(_ object: [String: Any]) -> UpdateMetadata {
        var description = object["itemDescription"] as? String
        if let feed = object["itemDescriptionFeed"] as? String { description = feedDescription(feed) }
        return UpdateMetadata(
            versionString: object["versionString"] as? String ?? "47",
            displayVersionString: object["displayVersionString"] as? String ?? "1.1.0",
            title: object["title"] as? String,
            itemDescription: description,
            releaseNotesURL: (object["releaseNotesURL"] as? String).flatMap { URL(string: $0) },
            infoURL: (object["infoURL"] as? String).flatMap { URL(string: $0) },
            contentLength: (object["contentLength"] as? NSNumber)?.uint64Value ?? 0,
            isInformationOnly: (object["isInformationOnly"] as? NSNumber)?.boolValue ?? false,
            isMajorUpgrade: false,
            isCritical: (object["isCritical"] as? NSNumber)?.boolValue ?? false,
            minimumSystemVersion: nil
        )
    }

    /// `NSError(domain:code:userInfo:)` from `{"domain", "code", "userInfo"}`.
    static func error(_ object: [String: Any]) -> NSError {
        NSError(
            domain: object["domain"] as? String ?? "sparkle",
            code: (object["code"] as? NSNumber)?.intValue ?? 0,
            userInfo: object["userInfo"] as? [String: Any]
        )
    }

    private static func bool(_ value: Any?, _ fallback: Bool) -> Bool {
        (value as? NSNumber)?.boolValue ?? fallback
    }

    private static func stage(_ value: Any?) -> UpdateStage {
        switch value as? String ?? "notDownloaded" {
        case "downloaded": return .downloaded
        case "installing": return .installing
        default: return .notDownloaded
        }
    }

    /// Applies driver callbacks (and fake-engine settings) in order. Every
    /// capability handed over is a no-op closure.
    static func run(_ steps: [Any], on c: UpdateCoordinator, engine: FakeUpdateEngine?) {
        for case let step as [String: Any] in steps {
            switch step["do"] as? String ?? "" {
            case "driverDidBeginUserCheck": c.driverDidBeginUserCheck(cancellation: {})
            case "driverDidFindUpdate":
                c.driverDidFindUpdate(
                    metadata(step["metadata"] as? [String: Any] ?? [:]), stage: stage(step["stage"]),
                    userInitiated: bool(step["userInitiated"], true), reply: { _ in }
                )
            case "driverDidReceiveReleaseNotes":
                c.driverDidReceiveReleaseNotes(Data((step["text"] as? String ?? "").utf8))
            case "driverDidFailToDownloadReleaseNotes":
                c.driverDidFailToDownloadReleaseNotes(NSError(domain: "sparkle", code: 0))
            case "driverDidFindNoUpdate":
                c.driverDidFindNoUpdate(userInitiated: bool(step["userInitiated"], false), acknowledgement: {})
            case "driverDidEncounterError":
                c.driverDidEncounterError(error(step["error"] as? [String: Any] ?? [:]), acknowledgement: {})
            case "driverDidBeginDownload": c.driverDidBeginDownload(cancellation: {})
            case "driverDidReceiveExpectedLength":
                c.driverDidReceiveExpectedLength((step["length"] as? NSNumber)?.uint64Value ?? 0)
            case "driverDidReceiveData": c.driverDidReceiveData((step["length"] as? NSNumber)?.uint64Value ?? 0)
            case "driverDidBeginExtraction": c.driverDidBeginExtraction()
            case "driverDidReceiveExtractionProgress":
                c.driverDidReceiveExtractionProgress((step["progress"] as? NSNumber)?.doubleValue ?? 0)
            case "driverDidBecomeReadyToRelaunch": c.driverDidBecomeReadyToRelaunch(reply: { _ in })
            case "driverDidBeginInstallation":
                c.driverDidBeginInstallation(
                    applicationTerminated: bool(step["applicationTerminated"], false), retryTermination: {}
                )
            case "completeBackgroundDownload":
                engine?.completeBackgroundDownload(displayVersion: step["version"] as? String ?? "")
            case "lastUpdateCheckDate":
                engine?.lastUpdateCheckDate = Date(
                    timeIntervalSinceReferenceDate: (step["value"] as? NSNumber)?.doubleValue ?? 0
                )
            case let other: fatalError("unknown update scene step \(other)")
            }
        }
    }

    static func json(_ pill: UpdatePillModel?) -> JSON {
        guard let pill else { return .null }
        switch pill {
        case .updateNow(let version, let isReady):
            return .object([("pill", .string("updateNow")), ("version", .string(version)), ("isReady", .bool(isReady))])
        case .restartToUpdate:
            return .object([("pill", .string("restartToUpdate"))])
        case .progress(let label, let fraction):
            return .object([
                ("pill", .string("progress")), ("label", .string(label)),
                ("fraction", fraction.map(JSON.double) ?? .null),
            ])
        case .warning:
            return .object([("pill", .string("warning"))])
        case .informational(let version):
            return .object([("pill", .string("informational")), ("version", .string(version))])
        }
    }

    /// Stops every progress indicator under `view`: a spinning indicator
    /// never settles, so a capture freezes it where it stands.
    static func stopIndicators(in view: NSView) {
        if let indicator = view as? NSProgressIndicator { indicator.stopAnimation(nil) }
        for subview in view.subviews { stopIndicators(in: subview) }
    }
}
