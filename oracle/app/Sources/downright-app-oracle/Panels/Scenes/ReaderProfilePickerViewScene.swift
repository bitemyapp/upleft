import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `ReaderProfilePickerView` scenes over an `InMemoryReaderProfileStore`
/// seeded with `state.custom` (`{"id", "name", "scale", "measure",
/// "density", "motion"}`, raw values); the JSON store and the support folder
/// are never touched. Then, in order: `state.select` (`selectProfile(id:)`),
/// `state.profileIndex` (the profile menu, picked and its action sent as a
/// click does), `state.controls` (`[menu, item]` pairs on the four metric
/// menus), `state.save` (`saveCustomProfileForTesting(name:)`; its id is a
/// fresh UUID and never dumped), `state.delete`, `state.revert` (the
/// buttons' actions). `state.current` builds with the default sheet and
/// assigns it afterwards. Reduce Motion: nothing here animates.
@MainActor
final class ReaderProfilePickerViewScene: PanelScene {
    private var picker: ReaderProfilePickerView?
    private var store: InMemoryReaderProfileStore?
    private var delegate: Delegate?

    private final class Delegate: ReaderProfilePickerDelegate {
        var events: [String] = []
        func readerProfilePicker(_ picker: ReaderProfilePickerView, didPreview profile: ReaderProfile) {
            events.append("preview \(profile.name)")
        }
        func readerProfilePicker(_ picker: ReaderProfilePickerView, didSelect profile: ReaderProfile) {
            events.append("select \(profile.name)")
        }
    }

    static func describe(_ profile: ReaderProfile) -> JSON {
        .object([
            ("name", .string(profile.name)),
            ("isBuiltIn", .bool(profile.isBuiltIn)),
            ("typographyScale", .string(profile.typographyScale.rawValue)),
            ("measureCharacters", .double(Double(profile.measureCharacters))),
            ("chromeDensity", .string(profile.chromeDensity.rawValue)),
            ("motionPreference", .string(profile.motionPreference.rawValue)),
        ])
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let custom: [ReaderProfile] = scenario.array("custom").compactMap { value in
            guard let object = value as? [String: Any] else { return nil }
            return ReaderProfile(
                id: object["id"] as? String ?? "",
                name: object["name"] as? String ?? "",
                typographyScale: ReaderTypographyScale(rawValue: object["scale"] as? String ?? "") ?? .standard,
                measureCharacters: CGFloat((object["measure"] as? NSNumber)?.doubleValue ?? 70),
                chromeDensity: ReaderChromeDensity(rawValue: object["density"] as? String ?? "") ?? .comfortable,
                motionPreference: ReaderMotionPreference(rawValue: object["motion"] as? String ?? "") ?? .followSystem
            )
        }
        let store = InMemoryReaderProfileStore(custom)
        let picker = scenario.bool("current")
            ? ReaderProfilePickerView(store: store)
            : ReaderProfilePickerView(store: store, styleSheet: styleSheet)
        if scenario.bool("current") { picker.styleSheet = styleSheet }
        let delegate = Delegate()
        picker.delegate = delegate
        self.delegate = delegate
        if let id = scenario.string("select") { picker.selectProfile(id: id) }
        let popups = picker.subviews.compactMap { $0 as? NSPopUpButton }
        if let index = scenario.int("profileIndex"), let popup = popups.first {
            popup.selectItem(at: index)
            popup.sendAction(popup.action, to: popup.target)
        }
        for pair in scenario.array("controls") {
            let values = (pair as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue }
            guard values.count == 2, values[0] + 1 < popups.count else { continue }
            let popup = popups[values[0] + 1]
            popup.selectItem(at: values[1])
            popup.sendAction(popup.action, to: popup.target)
        }
        if let name = scenario.string("save") { picker.saveCustomProfileForTesting(name: name) }
        let buttons = picker.subviews.compactMap { $0 as? NSButton }.filter { !($0 is NSPopUpButton) }
        let rows = picker.subviews.compactMap { $0 as? NSStackView }.flatMap { $0.arrangedSubviews.compactMap { $0 as? NSButton } }
        if scenario.bool("delete"), let button = buttons.first(where: { $0.title == "Delete Custom" }) {
            button.sendAction(button.action, to: button.target)
        }
        if scenario.bool("revert"), let button = rows.first(where: { $0.title == "Revert" }) {
            button.sendAction(button.action, to: button.target)
        }
        self.picker = picker
        self.store = store
        return picker
    }

    func model() -> JSON {
        guard let picker else { return .null }
        return .object([
            ("preferredWidth", .double(Double(picker.preferredWidth))),
            ("selectedProfile", Self.describe(picker.selectedProfile)),
            ("profiles", .array(picker.profiles.map { .string($0.name) })),
            ("stored", .array((store?.profiles ?? []).map { Self.describe($0) })),
            ("events", .array((delegate?.events ?? []).map { .string($0) })),
            ("fittingSize", PanelTree.size(picker.fittingSize)),
        ])
    }
}
