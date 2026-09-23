import AppKit
import Foundation
import Sparkle
@testable import DownrightApp

/// Swift side of the `updater` suite (`crates/conformance/src/dump/updater.rs`
/// is the Rust side). A case file is a JSON object whose `kind` selects one of:
///
/// - `state-machine-table`: every (state, event) pair of `UpdateStateMachine`.
///   States are reached by event sequences; each row is the phase after one
///   more event and whether the machine still equals the one before it
///   (Swift `Equatable`, so NaN progress and canonically equivalent strings
///   are exercised).
/// - `coordinator`: scripted `UpdateCoordinator` sessions over a
///   `FakeUpdateEngine`, with a snapshot of all derived state (phase, pill,
///   pending update, flags, counts, release notes, settings, status line)
///   and every side effect (capability calls, `stateDidChange` posts) after
///   each step.
/// - `coordinator-table`: every (setup, action) pair of the coordinator.
/// - `release-watch`: `ReleaseWatchPolicy` for every input combination, and
///   scripted `ReleaseWatch` sessions over a scripted probe, optionally wired
///   to a coordinator.
/// - `feed-probe`: `ReleaseFeedURLProbe` against a stub `URLProtocol` that
///   serves each sample appcast under scripted responses. Downright never
///   parses an appcast (Sparkle does); what it derives from a feed is this
///   probe's answer: the validator (ETag, Last-Modified, or a SHA-256 of the
///   body), the conditional headers it sends, and changed/unchanged/unreachable.
/// - `configuration`: `UpdateConfiguration.isValid(infoDictionary:)`.
/// - `failure`: `UpdateFailure(error:)`.
/// - `driver`: scripted `DownrightUpdateDriver` callbacks forwarded to a
///   coordinator, with what Sparkle's reply blocks receive.
///
/// Clocks: `ReleaseWatch` reads `Date()` for its 20 s spacing floor and has
/// no injection point. No timestamp reaches the output: every script runs in
/// well under 20 s, so the floor applies to every event after the first
/// probe unless `probeNow` resets it, on both sides. The coordinator's only
/// date, `lastUpdateCheckDate`, is injected through `FakeUpdateEngine`.
/// `ReleaseWatch.start` reads the live Low Power Mode state and `NSApp`
/// (nil here), identically on both sides.
///
/// Nothing reaches the network: probes use a session whose only protocol
/// class is the stub, and feed URLs are under `.invalid`. Nothing opens a
/// window, an alert, a URL or a beep: coordinators run with
/// `suppressUIForTesting`, and a press of "Update Now" in the informational
/// phase (which opens the info URL or beeps, with no seam) is reported as
/// skipped instead of performed.
enum UpdaterDump {
    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let kind = root["kind"] as? String
        else {
            throw AppOracleError(description: "updater case needs a `kind`")
        }
        let directory = input.deletingLastPathComponent()
        return try MainActor.assumeIsolated {
            let fixtures = Fixtures(root["metadata"] as? [String: Any] ?? [:])
            switch kind {
            case "state-machine-table": return stateMachineTable(root, fixtures)
            case "coordinator": return coordinatorScripts(root, fixtures)
            case "coordinator-table": return coordinatorTable(root, fixtures)
            case "release-watch": return releaseWatch(root, fixtures)
            case "feed-probe": return try feedProbe(root, directory: directory)
            case "configuration": return configuration(root)
            case "failure": return failure(root)
            case "driver": return driver(root, fixtures)
            default: throw AppOracleError(description: "unknown updater case kind \(kind)")
            }
        }
    }
}

// MARK: - Values

private final class Box<T>: @unchecked Sendable {
    var value: T
    init(_ value: T) { self.value = value }
}

private func u64(_ value: Any?) -> UInt64 {
    if let string = value as? String { return UInt64(string)! }
    return (value as! NSNumber).uint64Value
}

private func double(_ value: Any?) -> Double {
    if let string = value as? String {
        switch string {
        case "nan": return .nan
        case "inf": return .infinity
        case "-inf": return -.infinity
        default: return Double(string)!
        }
    }
    return (value as! NSNumber).doubleValue
}

private func bool(_ value: Any?, _ fallback: Bool) -> Bool {
    (value as? NSNumber)?.boolValue ?? fallback
}

private struct Fixtures {
    var named: [String: Any]
    init(_ named: [String: Any]) { self.named = named }

    /// A metadata value: the name of a fixture, or an inline object.
    func metadata(_ value: Any?) -> UpdateMetadata {
        let object = (value as? String).map { named[$0] as! [String: Any] } ?? (value as! [String: Any])
        return UpdateMetadata(
            versionString: object["versionString"] as! String,
            displayVersionString: object["displayVersionString"] as! String,
            title: object["title"] as? String,
            itemDescription: object["itemDescription"] as? String,
            releaseNotesURL: (object["releaseNotesURL"] as? String).flatMap { URL(string: $0) },
            infoURL: (object["infoURL"] as? String).flatMap { URL(string: $0) },
            contentLength: object["contentLength"].map(u64) ?? 0,
            isInformationOnly: bool(object["isInformationOnly"], false),
            isMajorUpgrade: bool(object["isMajorUpgrade"], false),
            isCritical: bool(object["isCritical"], false),
            minimumSystemVersion: object["minimumSystemVersion"] as? String
        )
    }
}

private func stage(_ value: Any?) -> UpdateStage {
    switch value as? String ?? "notDownloaded" {
    case "downloaded": return .downloaded
    case "installing": return .installing
    default: return .notDownloaded
    }
}

private func failureValue(_ value: Any?) -> UpdateFailure {
    if let name = value as? String, name == "generic" { return .generic }
    let object = value as! [String: Any]
    return UpdateFailure(
        message: object["message"] as! String,
        technicalDetail: object["technicalDetail"] as? String,
        code: (object["code"] as! NSNumber).intValue,
        retryable: bool(object["retryable"], true)
    )
}

/// `NSError(domain:code:userInfo:)` from `{"domain", "code", "userInfo"}`.
private func errorValue(_ value: Any?) -> NSError {
    let object = value as? [String: Any] ?? [:]
    return NSError(
        domain: object["domain"] as? String ?? "sparkle",
        code: (object["code"] as? NSNumber)?.intValue ?? 0,
        userInfo: object["userInfo"] as? [String: Any]
    )
}

private func event(_ object: [String: Any], _ fixtures: Fixtures) -> UpdateStateMachine.UpdateEvent {
    switch object["event"] as! String {
    case "userInitiatedCheckBegan": return .userInitiatedCheckBegan
    case "automaticCheckBegan": return .automaticCheckBegan
    case "updateFound": return .updateFound(fixtures.metadata(object["metadata"]), stage: stage(object["stage"]))
    case "releaseNotesAvailable": return .releaseNotesAvailable
    case "releaseNotesFailed": return .releaseNotesFailed
    case "updateNotFound": return .updateNotFound(userInitiated: bool(object["userInitiated"], false))
    case "updaterError": return .updaterError(failureValue(object["failure"]))
    case "downloadInitiated": return .downloadInitiated
    case "expectedLength": return .expectedLength(u64(object["length"]))
    case "dataReceived": return .dataReceived(u64(object["length"]))
    case "extractionBegan": return .extractionBegan
    case "extractionProgress": return .extractionProgress(double(object["progress"]))
    case "readyToInstallAndRelaunch": return .readyToInstallAndRelaunch
    case "installingUpdate": return .installingUpdate(applicationTerminated: bool(object["applicationTerminated"], false))
    case "updateInstalled": return .updateInstalled(relaunched: bool(object["relaunched"], false))
    case "dismissed": return .dismissed
    case "checkCancelled": return .checkCancelled
    case "downloadCancelled": return .downloadCancelled
    case let other: fatalError("unknown event \(other)")
    }
}

// MARK: - Output

private func json(_ string: String?) -> JSON { string.map(JSON.string) ?? .null }

private func json(_ metadata: UpdateMetadata?) -> JSON {
    guard let metadata else { return .null }
    return .object([
        ("versionString", .string(metadata.versionString)),
        ("displayVersionString", .string(metadata.displayVersionString)),
        ("title", json(metadata.title)),
        ("itemDescription", json(metadata.itemDescription)),
        ("releaseNotesURL", json(metadata.releaseNotesURL?.absoluteString)),
        ("infoURL", json(metadata.infoURL?.absoluteString)),
        ("contentLength", .string(String(metadata.contentLength))),
        ("isInformationOnly", .bool(metadata.isInformationOnly)),
        ("isMajorUpgrade", .bool(metadata.isMajorUpgrade)),
        ("isCritical", .bool(metadata.isCritical)),
        ("minimumSystemVersion", json(metadata.minimumSystemVersion)),
    ])
}

private func json(_ failure: UpdateFailure) -> JSON {
    .object([
        ("message", .string(failure.message)),
        ("technicalDetail", json(failure.technicalDetail)),
        ("code", .int(failure.code)),
        ("retryable", .bool(failure.retryable)),
    ])
}

private func json(_ stage: UpdateStage) -> JSON {
    switch stage {
    case .notDownloaded: return .string("notDownloaded")
    case .downloaded: return .string("downloaded")
    case .installing: return .string("installing")
    }
}

private func json(_ phase: UpdateStateMachine.UpdatePhase) -> JSON {
    switch phase {
    case .idle: return .object([("phase", .string("idle"))])
    case .checking(let userInitiated):
        return .object([("phase", .string("checking")), ("userInitiated", .bool(userInitiated))])
    case .available(let metadata, let stage):
        return .object([("phase", .string("available")), ("metadata", json(metadata)), ("stage", json(stage))])
    case .downloading(let received, let expected):
        return .object([
            ("phase", .string("downloading")),
            ("received", .string(String(received))),
            ("expected", expected.map { .string(String($0)) } ?? .null),
        ])
    case .extracting(let progress):
        return .object([("phase", .string("extracting")), ("progress", progress.map(JSON.double) ?? .null)])
    case .readyToRelaunch: return .object([("phase", .string("readyToRelaunch"))])
    case .waitingForTermination: return .object([("phase", .string("waitingForTermination"))])
    case .installing: return .object([("phase", .string("installing"))])
    case .informational(let metadata):
        return .object([("phase", .string("informational")), ("metadata", json(metadata))])
    case .upToDate: return .object([("phase", .string("upToDate"))])
    case .failed(let failure, let retryable):
        return .object([("phase", .string("failed")), ("failure", json(failure)), ("retryable", .bool(retryable))])
    }
}

private func json(_ pill: UpdatePillModel?) -> JSON {
    guard let pill else { return .null }
    var members: [(String, JSON)]
    switch pill {
    case .updateNow(let version, let isReady):
        members = [("pill", .string("updateNow")), ("version", .string(version)), ("isReady", .bool(isReady))]
    case .restartToUpdate:
        members = [("pill", .string("restartToUpdate"))]
    case .progress(let label, let fraction):
        members = [("pill", .string("progress")), ("label", .string(label)), ("fraction", fraction.map(JSON.double) ?? .null)]
    case .warning:
        members = [("pill", .string("warning"))]
    case .informational(let version):
        members = [("pill", .string("informational")), ("version", .string(version))]
    }
    members.append(("offersInstall", .bool(pill.offersInstall)))
    return .object(members)
}

private func json(_ notes: UpdateReleaseNotesState) -> JSON {
    switch notes {
    case .none: return .object([("state", .string("none"))])
    case .loaded(let data):
        return .object([("state", .string("loaded")), ("hex", .string(data.map { String(format: "%02x", $0) }.joined()))])
    case .failed: return .object([("state", .string("failed"))])
    }
}

private func json(_ result: ReleaseFeedProbeResult?) -> JSON {
    guard let result else { return .null }
    switch result {
    case .unchanged: return .object([("result", .string("unchanged"))])
    case .changed(let validator): return .object([("result", .string("changed")), ("validator", json(validator))])
    case .unreachable: return .object([("result", .string("unreachable"))])
    }
}

private func name(_ choice: UpdateUserChoice) -> String {
    switch choice {
    case .install: return "install"
    case .later: return "later"
    case .skip: return "skip"
    }
}

/// Runs the main run loop until `done` holds (bounded).
private func pump(_ done: () -> Bool) {
    let deadline = Date().addingTimeInterval(20)
    while !done() && Date() < deadline {
        CFRunLoopRunInMode(.defaultMode, 0.01, true)
    }
}

/// Lets every main-queue block enqueued so far run: the queue is FIFO, so a
/// sentinel enqueued now runs after all of them.
private func drainMainQueue() {
    let done = Box(false)
    DispatchQueue.main.async { done.value = true }
    pump { done.value }
}

// MARK: - State machine table

@MainActor
private func stateMachineTable(_ root: [String: Any], _ fixtures: Fixtures) -> JSON {
    let events = root["events"] as! [[String: Any]]
    var states: [JSON] = []
    for state in root["states"] as! [[String: Any]] {
        var machine = UpdateStateMachine()
        for step in state["events"] as! [[String: Any]] { machine.reduce(event(step, fixtures)) }
        var rows: [JSON] = []
        for step in events {
            var next = machine
            next.reduce(event(step, fixtures))
            rows.append(.object([("phase", json(next.phase)), ("equalsBefore", .bool(next == machine))]))
        }
        states.append(.object([
            ("state", .string(state["name"] as! String)),
            ("phase", json(machine.phase)),
            ("rows", .array(rows)),
        ]))
    }
    return .object([("states", .array(states))])
}

// MARK: - Coordinator

@MainActor
private final class CoordinatorHarness {
    let engine: FakeUpdateEngine?
    let coordinator: UpdateCoordinator
    let fixtures: Fixtures
    var effects: [String] = []
    var notifications = 0
    private var token: NSObjectProtocol?

    /// `engine`: `"started"` (the tests' `makeCoordinator`), `"stopped"`, or `"none"`.
    init(engine mode: String, fixtures: Fixtures) {
        self.fixtures = fixtures
        let engine: FakeUpdateEngine? = mode == "none" ? nil : FakeUpdateEngine()
        self.engine = engine
        coordinator = UpdateCoordinator(engine: engine)
        coordinator.suppressUIForTesting = true
        if mode == "started" { try? engine?.start() }
        token = NotificationCenter.default.addObserver(
            forName: UpdateCoordinator.stateDidChange, object: coordinator, queue: nil
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.notifications += 1 }
        }
    }

    func close() {
        if let token { NotificationCenter.default.removeObserver(token) }
        token = nil
    }

    func callback(_ label: String) -> () -> Void {
        { [weak self] in self?.effects.append(label) }
    }

    func reply(_ label: String) -> (UpdateUserChoice) -> Void {
        { [weak self] choice in self?.effects.append("\(label):\(name(choice))") }
    }

    /// Applies one step; answers a note when the step was skipped.
    func apply(_ step: [String: Any], index: Int) -> String? {
        let c = coordinator
        switch step["do"] as! String {
        case "driverDidBeginUserCheck": c.driverDidBeginUserCheck(cancellation: callback("checkCancel#\(index)"))
        case "driverDidFindUpdate":
            c.driverDidFindUpdate(
                fixtures.metadata(step["metadata"]), stage: stage(step["stage"]),
                userInitiated: bool(step["userInitiated"], true), reply: reply("choiceReply#\(index)")
            )
        case "driverDidReceiveReleaseNotes": c.driverDidReceiveReleaseNotes(Data((step["text"] as! String).utf8))
        case "driverDidFailToDownloadReleaseNotes": c.driverDidFailToDownloadReleaseNotes(errorValue(step["error"]))
        case "driverDidFindNoUpdate":
            c.driverDidFindNoUpdate(userInitiated: bool(step["userInitiated"], false), acknowledgement: callback("ack#\(index)"))
        case "driverDidEncounterError":
            c.driverDidEncounterError(errorValue(step["error"]), acknowledgement: callback("ack#\(index)"))
        case "driverDidBeginDownload": c.driverDidBeginDownload(cancellation: callback("downloadCancel#\(index)"))
        case "driverDidReceiveExpectedLength": c.driverDidReceiveExpectedLength(u64(step["length"]))
        case "driverDidReceiveData": c.driverDidReceiveData(u64(step["length"]))
        case "driverDidBeginExtraction": c.driverDidBeginExtraction()
        case "driverDidReceiveExtractionProgress": c.driverDidReceiveExtractionProgress(double(step["progress"]))
        case "driverDidBecomeReadyToRelaunch": c.driverDidBecomeReadyToRelaunch(reply: reply("readyReply#\(index)"))
        case "driverDidBeginInstallation":
            c.driverDidBeginInstallation(
                applicationTerminated: bool(step["applicationTerminated"], false),
                retryTermination: callback("retryTermination#\(index)")
            )
        case "driverDidFinishInstallation":
            c.driverDidFinishInstallation(relaunched: bool(step["relaunched"], true), acknowledgement: callback("ack#\(index)"))
        case "driverDidDismiss": c.driverDidDismiss()
        case "driverDidRequestFocus": c.driverDidRequestFocus()
        case "userDidPressUpdateNow":
            if case .informational = c.phase { return "skipped: opens the info URL or beeps" }
            c.userDidPressUpdateNow()
        case "userDidChooseInstall": c.userDidChooseInstall()
        case "userDidRetry": c.userDidRetry()
        case "userDidChooseLater": c.userDidChooseLater()
        case "userDidChooseSkip": c.userDidChooseSkip()
        case "userDidCancelCheck": c.userDidCancelCheck()
        case "userDidCancelDownload": c.userDidCancelDownload()
        case "userDidRetryTermination": c.userDidRetryTermination()
        case "userDidAcknowledge": c.userDidAcknowledge()
        case "userDidDismissPanel": c.userDidDismissPanel()
        case "checkForUpdates": c.checkForUpdates()
        case "showPanel": c.showPanel()
        case "closePanel": c.closePanel()
        case "start": c.start()
        case "tearDownForTesting": c.tearDownForTesting()
        case "releaseFeedDidChange": c.releaseFeedDidChange()
        case "setAutomaticallyChecksForUpdates": c.automaticallyChecksForUpdates = bool(step["value"], true)
        case "setAutomaticallyDownloadsUpdates": c.automaticallyDownloadsUpdates = bool(step["value"], true)
        case "completeBackgroundDownload": engine?.completeBackgroundDownload(displayVersion: step["version"] as! String)
        case "engineStart":
            do { try engine?.start() } catch {
                let error = error as NSError
                effects.append("engineStartThrew:\(error.domain):\(error.code)")
            }
        case "engineSet":
            guard let engine else { return "skipped: no engine" }
            let values = step["values"] as! [String: Any]
            if let value = values["startThrows"] { engine.startThrows = bool(value, false) }
            if let value = values["canCheckForUpdates"] { engine._canCheckForUpdates = bool(value, true) }
            if let value = values["automaticallyChecksForUpdates"] { engine.automaticallyChecksForUpdates = bool(value, true) }
            if let value = values["automaticallyDownloadsUpdates"] { engine.automaticallyDownloadsUpdates = bool(value, true) }
            if let value = values["allowsAutomaticUpdates"] { engine.allowsAutomaticUpdates = bool(value, true) }
            if let value = values["updateCheckInterval"] { engine.updateCheckInterval = double(value) }
            if let value = values["lastUpdateCheckDate"] {
                engine.lastUpdateCheckDate = value is NSNull ? nil : Date(timeIntervalSinceReferenceDate: double(value))
            }
        case "drain": drainMainQueue()
        case let other: fatalError("unknown coordinator step \(other)")
        }
        return nil
    }

    /// Everything observable about the coordinator, plus the effects and
    /// notifications since the last snapshot.
    func snapshot() -> JSON {
        let c = coordinator
        var members: [(String, JSON)] = [
            ("phase", json(c.phase)),
            ("pill", json(c.pillModel)),
            ("pendingUpdate", json(c.pendingUpdate)),
            ("downloadedUpdate", json(c.downloadedUpdate)),
            ("currentCycleUpdate", json(c.currentCycleUpdate)),
            ("currentCycleIsUserInitiated", .bool(c.currentCycleIsUserInitiated)),
            ("isExpeditedInstall", .bool(c.isExpeditedInstall)),
            ("panelShowCount", .int(c.panelShowCount)),
            ("releaseWatchTriggerCount", .int(c.releaseWatchTriggerCount)),
            ("releaseNotes", json(c.releaseNotes)),
            ("isRunning", .bool(c.isRunning)),
            ("canCheckForUpdates", .bool(c.canCheckForUpdates)),
            ("isUpdateConfigurationPresent", .bool(c.isUpdateConfigurationPresent)),
            ("automaticallyChecksForUpdates", .bool(c.automaticallyChecksForUpdates)),
            ("automaticallyDownloadsUpdates", .bool(c.automaticallyDownloadsUpdates)),
            ("allowsAutomaticUpdates", .bool(c.allowsAutomaticUpdates)),
            ("lastUpdateCheckDate", c.lastUpdateCheckDate.map { .double($0.timeIntervalSinceReferenceDate) } ?? .null),
            ("statusLine", .string(c.statusLine())),
        ]
        if let engine {
            members.append(("engine", .object([
                ("isRunning", .bool(engine.isRunning)),
                ("foregroundCheckCount", .int(engine.foregroundCheckCount)),
                ("backgroundCheckCount", .int(engine.backgroundCheckCount)),
                ("automaticallyChecksForUpdates", .bool(engine.automaticallyChecksForUpdates)),
                ("automaticallyDownloadsUpdates", .bool(engine.automaticallyDownloadsUpdates)),
                ("hasBackgroundHandler", .bool(engine.onBackgroundDownloadCompleted != nil)),
            ])))
        } else {
            members.append(("engine", .null))
        }
        members.append(("notifications", .int(notifications)))
        members.append(("effects", .array(effects.map(JSON.string))))
        effects = []
        notifications = 0
        return .object(members)
    }

    /// Runs `steps`, answering one entry (with a snapshot) per step.
    func run(_ steps: [[String: Any]], from start: Int = 0) -> [JSON] {
        steps.enumerated().map { offset, step in
            let note = apply(step, index: start + offset)
            var entry: [(String, JSON)] = [("do", .string(step["do"] as! String))]
            if let note { entry.append(("note", .string(note))) }
            entry.append(("after", snapshot()))
            return .object(entry)
        }
    }
}

@MainActor
private func coordinatorScripts(_ root: [String: Any], _ fixtures: Fixtures) -> JSON {
    var scripts: [JSON] = []
    for script in root["scripts"] as! [[String: Any]] {
        let harness = CoordinatorHarness(engine: script["engine"] as? String ?? "started", fixtures: fixtures)
        let initial = harness.snapshot()
        let steps = harness.run(script["steps"] as! [[String: Any]])
        harness.close()
        scripts.append(.object([
            ("name", .string(script["name"] as! String)),
            ("initial", initial),
            ("steps", .array(steps)),
        ]))
    }
    return .object([("scripts", .array(scripts))])
}

/// Each setup once with a snapshot per step, then every action from a fresh
/// coordinator brought to that setup.
@MainActor
private func coordinatorTable(_ root: [String: Any], _ fixtures: Fixtures) -> JSON {
    let actions = root["actions"] as! [[String: Any]]
    var setups: [JSON] = []
    for setup in root["setups"] as! [[String: Any]] {
        let engine = setup["engine"] as? String ?? "started"
        let steps = setup["steps"] as! [[String: Any]]
        let reference = CoordinatorHarness(engine: engine, fixtures: fixtures)
        let initial = reference.snapshot()
        let setupSteps = reference.run(steps)
        reference.close()
        var rows: [JSON] = []
        for action in actions {
            let harness = CoordinatorHarness(engine: engine, fixtures: fixtures)
            _ = harness.run(steps)
            rows.append(harness.run([action], from: steps.count)[0])
            harness.close()
        }
        setups.append(.object([
            ("setup", .string(setup["name"] as! String)),
            ("initial", initial),
            ("steps", .array(setupSteps)),
            ("rows", .array(rows)),
        ]))
    }
    return .object([("setups", .array(setups))])
}

// MARK: - Release watch

@MainActor
private final class ScriptedFeedProbe: ReleaseFeedProbe {
    var results: [ReleaseFeedProbeResult]
    var validators: [String?] = []

    init(_ results: [ReleaseFeedProbeResult]) { self.results = results }

    func probe(feed: URL, validator: String?) async -> ReleaseFeedProbeResult {
        validators.append(validator)
        return results.isEmpty ? .unchanged : results.removeFirst()
    }
}

private func probeResult(_ object: [String: Any]) -> ReleaseFeedProbeResult {
    switch object["result"] as! String {
    case "unchanged": return .unchanged
    case "changed": return .changed(validator: object["validator"] as? String)
    default: return .unreachable
    }
}

private func json(_ policy: ReleaseWatchPolicy) -> JSON {
    .object([
        ("isAppActive", .bool(policy.isAppActive)),
        ("isLowPower", .bool(policy.isLowPower)),
        ("hasNetwork", .bool(policy.hasNetwork)),
        ("interval", policy.interval.map(JSON.double) ?? .null),
    ])
}

@MainActor
private func releaseWatch(_ root: [String: Any], _ fixtures: Fixtures) -> JSON {
    var policies: [JSON] = []
    for active in [false, true] {
        for lowPower in [false, true] {
            for network in [false, true] {
                policies.append(json(ReleaseWatchPolicy(isAppActive: active, isLowPower: lowPower, hasNetwork: network)))
            }
        }
    }
    let constants = JSON.object([
        ("activeInterval", .double(ReleaseWatchPolicy.activeInterval)),
        ("backgroundInterval", .double(ReleaseWatchPolicy.backgroundInterval)),
        ("minimumSpacing", .double(ReleaseWatchPolicy.minimumSpacing)),
        ("default", json(ReleaseWatchPolicy())),
    ])

    var scripts: [JSON] = []
    for script in root["scripts"] as? [[String: Any]] ?? [] {
        let probe = ScriptedFeedProbe((script["results"] as? [[String: Any]] ?? []).map(probeResult))
        let watch = ReleaseWatch(feed: URL(string: "https://feeds.example.invalid/appcast.xml")!, prober: probe)
        let fired = Box(0)
        watch.onFeedChanged = { fired.value += 1 }
        var harness: CoordinatorHarness?
        var entries: [JSON] = []
        for (index, step) in (script["steps"] as! [[String: Any]]).enumerated() {
            var note: String?
            switch step["do"] as! String {
            case "start": watch.start(observingSystemEvents: false)
            case "stop": watch.stop()
            case "probeNow": watch.probeNow()
            case "systemDidWake": watch.systemDidWake()
            case "activation": watch.applicationDidChangeActivation(isActive: bool(step["isActive"], true))
            case "powerState": watch.powerStateDidChange(isLowPower: bool(step["isLowPower"], false))
            case "network": watch.networkAvailabilityDidChange(hasNetwork: bool(step["hasNetwork"], true))
            case "drain": drainMainQueue()
            case "wireCoordinator":
                // The coordinator's own wiring: the watch asks Sparkle to look.
                let wired = CoordinatorHarness(engine: step["engine"] as? String ?? "started", fixtures: fixtures)
                harness = wired
                watch.onFeedChanged = { [weak wired] in
                    fired.value += 1
                    wired?.coordinator.releaseFeedDidChange()
                }
            case "coordinator":
                if let harness {
                    note = harness.apply(step["step"] as! [String: Any], index: index)
                } else {
                    note = "skipped: no coordinator"
                }
            case let other: fatalError("unknown release-watch step \(other)")
            }
            var members: [(String, JSON)] = [("do", .string(step["do"] as! String))]
            if let note { members.append(("note", .string(note))) }
            members += [
                ("completedProbeCount", .int(watch.completedProbeCount)),
                ("lastResult", json(watch.lastResult)),
                ("hasBaseline", .bool(watch.hasBaseline)),
                ("policy", json(watch.policy)),
                ("fired", .int(fired.value)),
                ("validators", .array(probe.validators.map(json))),
            ]
            if let harness { members.append(("coordinator", harness.snapshot())) }
            entries.append(.object(members))
        }
        watch.stop()
        harness?.close()
        scripts.append(.object([("name", .string(script["name"] as! String)), ("steps", .array(entries))]))
    }
    return .object([("constants", constants), ("policies", .array(policies)), ("scripts", .array(scripts))])
}

// MARK: - Feed probe

/// What the stub answers for one request.
private struct StubSpec {
    var kind: String  // "http", "nonHTTP", "error"
    var status: Int
    var headers: [String: String]
    var body: Data
}

private final class UpdaterStubURLProtocol: URLProtocol {
    nonisolated(unsafe) static var spec: StubSpec?
    nonisolated(unsafe) static var seen: [JSON] = []

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.seen.append(.object([
            ("method", json(request.httpMethod)),
            ("ifNoneMatch", json(request.value(forHTTPHeaderField: "If-None-Match"))),
            ("ifModifiedSince", json(request.value(forHTTPHeaderField: "If-Modified-Since"))),
            ("cachePolicy", .int(Int(request.cachePolicy.rawValue))),
        ]))
        guard let spec = Self.spec, let url = request.url else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse))
            return
        }
        switch spec.kind {
        case "error":
            client?.urlProtocol(self, didFailWithError: URLError(.notConnectedToInternet))
            return
        case "nonHTTP":
            let response = URLResponse(url: url, mimeType: "application/xml", expectedContentLength: spec.body.count, textEncodingName: nil)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        default:
            let response = HTTPURLResponse(url: url, statusCode: spec.status, httpVersion: "HTTP/1.1", headerFields: spec.headers)!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        }
        client?.urlProtocol(self, didLoad: spec.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

private func body(_ value: Any?, feed: Data) -> Data {
    if let name = value as? String {
        switch name {
        case "feed": return feed
        case "empty": return Data()
        default: fatalError("unknown body \(name)")
        }
    }
    guard let object = value as? [String: Any] else { return feed }
    if let text = object["text"] as? String { return Data(text.utf8) }
    if let repeated = object["repeat"] as? [String: Any] {
        let byte = UInt8((repeated["byte"] as! NSNumber).intValue)
        return Data(repeating: byte, count: (repeated["count"] as! NSNumber).intValue)
    }
    return feed
}

@MainActor
private func feedProbe(_ root: [String: Any], directory: URL) throws -> JSON {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [UpdaterStubURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let feedURL = URL(string: "https://feeds.example.invalid/appcast.xml")!
    let scenarios = root["scenarios"] as! [[String: Any]]

    var feeds: [JSON] = []
    for path in root["feeds"] as! [String] {
        let feed = try Data(contentsOf: directory.appendingPathComponent(path))
        var results: [JSON] = []
        for scenario in scenarios {
            let probe = ReleaseFeedURLProbe(session: session)
            var held: String?
            var probes: [JSON] = []
            for step in scenario["probes"] as! [[String: Any]] {
                let response = step["response"] as? [String: Any] ?? [:]
                UpdaterStubURLProtocol.spec = StubSpec(
                    kind: response["kind"] as? String ?? "http",
                    status: (response["status"] as? NSNumber)?.intValue ?? 200,
                    headers: response["headers"] as? [String: String] ?? [:],
                    body: body(response["body"], feed: feed)
                )
                UpdaterStubURLProtocol.seen = []
                let offered: String?
                switch step["offer"] {
                case let string as String: offered = string
                case let object as [String: Any] where object["held"] != nil: offered = held
                default: offered = nil
                }
                let answer = Box<ReleaseFeedProbeResult?>(nil)
                Task { @MainActor in answer.value = await probe.probe(feed: feedURL, validator: offered) }
                pump { answer.value != nil }
                if case .changed(let validator) = answer.value { held = validator }
                probes.append(.object([
                    ("offered", json(offered)),
                    ("result", json(answer.value)),
                    ("requests", .array(UpdaterStubURLProtocol.seen)),
                ]))
            }
            results.append(.object([("scenario", .string(scenario["name"] as! String)), ("probes", .array(probes))]))
        }
        feeds.append(.object([
            ("feed", .string(path)),
            ("bytes", .int(feed.count)),
            ("scenarios", .array(results)),
        ]))
    }
    return .object([("feeds", .array(feeds))])
}

// MARK: - Configuration and failures

private func configuration(_ root: [String: Any]) -> JSON {
    let cases = root["cases"] as! [[String: Any]]
    return .object([("cases", .array(cases.map { entry in
        .object([
            ("name", .string(entry["name"] as! String)),
            ("isValid", .bool(UpdateConfiguration.isValid(infoDictionary: entry["info"] as? [String: Any] ?? [:]))),
        ])
    }))])
}

private func failure(_ root: [String: Any]) -> JSON {
    var entries: [JSON] = [
        .object([("name", .string("generic")), ("failure", json(UpdateFailure.generic))]),
        .object([
            ("name", .string("UpdateStartError.updaterRefusedToStart")),
            ("failure", json(UpdateFailure(error: UpdateStartError.updaterRefusedToStart))),
        ]),
    ]
    for entry in root["errors"] as! [[String: Any]] {
        let failure = UpdateFailure(error: errorValue(entry["error"]))
        var machine = UpdateStateMachine()
        machine.reduce(.updaterError(failure))
        entries.append(.object([
            ("name", .string(entry["name"] as! String)),
            ("failure", json(failure)),
            ("phase", json(machine.phase)),
        ]))
    }
    return .object([("failures", .array(entries))])
}

// MARK: - Driver

@MainActor
private func driver(_ root: [String: Any], _ fixtures: Fixtures) -> JSON {
    var scripts: [JSON] = []
    for script in root["scripts"] as! [[String: Any]] {
        let harness = CoordinatorHarness(engine: script["engine"] as? String ?? "started", fixtures: fixtures)
        let driver = DownrightUpdateDriver(host: harness.coordinator)
        var entries: [JSON] = []
        for (index, step) in (script["steps"] as! [[String: Any]]).enumerated() {
            var note: String?
            let sparkleReply: (SPUUserUpdateChoice) -> Void = { [weak harness] choice in
                harness?.effects.append("sparkleReply#\(index):\(choice.rawValue)")
            }
            switch step["do"] as! String {
            case "permissionRequest":
                driver.show(SPUUpdatePermissionRequest(systemProfile: [])) { [weak harness] response in
                    let downloading = response.automaticUpdateDownloading.map { $0.boolValue ? "true" : "false" } ?? "nil"
                    harness?.effects.append(
                        "permission#\(index):checks=\(response.automaticUpdateChecks),downloading=\(downloading),profile=\(response.sendSystemProfile)"
                    )
                }
            case "showUserInitiatedUpdateCheck":
                driver.showUserInitiatedUpdateCheck(cancellation: harness.callback("sparkleCancel#\(index)"))
            case "showUpdateReleaseNotesFailedToDownloadWithError":
                driver.showUpdateReleaseNotesFailedToDownloadWithError(errorValue(step["error"]))
            case "showUpdateNotFoundWithError":
                driver.showUpdateNotFoundWithError(errorValue(step["error"]), acknowledgement: harness.callback("sparkleAck#\(index)"))
            case "showUpdaterError":
                driver.showUpdaterError(errorValue(step["error"]), acknowledgement: harness.callback("sparkleAck#\(index)"))
            case "showDownloadInitiated":
                driver.showDownloadInitiated(cancellation: harness.callback("sparkleCancel#\(index)"))
            case "showDownloadDidReceiveExpectedContentLength":
                driver.showDownloadDidReceiveExpectedContentLength(u64(step["length"]))
            case "showDownloadDidReceiveData": driver.showDownloadDidReceiveData(ofLength: u64(step["length"]))
            case "showDownloadDidStartExtractingUpdate": driver.showDownloadDidStartExtractingUpdate()
            case "showExtractionReceivedProgress": driver.showExtractionReceivedProgress(double(step["progress"]))
            case "showReadyToInstallAndRelaunch": driver.showReady(toInstallAndRelaunch: sparkleReply)
            case "showInstallingUpdate":
                driver.showInstallingUpdate(
                    withApplicationTerminated: bool(step["applicationTerminated"], false),
                    retryTerminatingApplication: harness.callback("sparkleRetry#\(index)")
                )
            case "showUpdateInstalledAndRelaunched":
                driver.showUpdateInstalledAndRelaunched(bool(step["relaunched"], true), acknowledgement: harness.callback("sparkleAck#\(index)"))
            case "dismissUpdateInstallation": driver.dismissUpdateInstallation()
            case "showUpdateInFocus": driver.showUpdateInFocus()
            case "detachHost": driver.host = nil
            case "coordinator": note = harness.apply(step["step"] as! [String: Any], index: index)
            case let other: fatalError("unknown driver step \(other)")
            }
            var members: [(String, JSON)] = [("do", .string(step["do"] as! String))]
            if let note { members.append(("note", .string(note))) }
            members.append(("after", harness.snapshot()))
            entries.append(.object(members))
        }
        harness.close()
        scripts.append(.object([("name", .string(script["name"] as! String)), ("steps", .array(entries))]))
    }
    return .object([("scripts", .array(scripts))])
}
