import AppKit
import Foundation
import MarkdownCore
@testable import DownrightApp

/// Swift side of the `palette` suite: the command table, key bindings and the
/// keybindings file, the command palette's ranking, Quick Open, the fuzzy
/// matcher, the recent-commands store, the welcome tour and the integration
/// policies. Mirrors `crates/conformance/src/dump/palette.rs`.
///
/// The input is a JSON object; each key present selects one section, and the
/// sections are dumped in a fixed order. `store` exercises the process-wide
/// `KeybindingStore.shared`, so a case holds at most one store scenario; its
/// support directory is a fresh temporary directory named by
/// `DOWNRIGHT_SUPPORT_DIRECTORY`, set before the store is first touched.
/// `recentStore` uses a unique `UserDefaults` suite, removed afterwards.
/// No field depends on the clock.
enum PaletteDump {
    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let spec = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AppOracleError(description: "palette input must be a JSON object")
        }
        var out: [(String, JSON)] = []
        if spec["table"] as? Bool == true { out.append(("table", table())) }
        if let contexts = spec["contexts"] as? [[String: Any]] {
            out.append(("contexts", .array(contexts.map(contextDump))))
        }
        if let strings = spec["parse"] as? [String] {
            out.append(("parse", .array(strings.map { KeyBinding(parsing: native($0)).map(bindingDump) ?? .null })))
        }
        if let snippets = spec["decodeBinding"] as? [String] {
            out.append(("decodeBinding", .array(snippets.map(decodeBindingDump))))
        }
        if let files = spec["decodeFile"] as? [String] {
            out.append(("decodeFile", .array(files.map { loadDump(KeybindingLoad.decode(Data(native($0).utf8))) })))
        }
        if let files = spec["decodeFileHex"] as? [String] {
            out.append(("decodeFileHex", .array(files.map { loadDump(KeybindingLoad.decode(Data(hexBytes($0)))) })))
        }
        if let stored = spec["encodeStored"] as? [[String: Any]] {
            out.append(("encodeStored", .array(stored.map(encodeStoredDump))))
        }
        if let pairs = spec["fuzzy"] as? [[String]] {
            out.append(("fuzzy", .array(pairs.map { fuzzyDump(needle: native($0[0]), haystack: native($0[1])) })))
        }
        if let searches = spec["search"] as? [[String: Any]] {
            out.append(("search", .array(searches.map { search in
                let query = native(search["query"] as? String ?? "")
                let candidates = (search["candidates"] as? [String] ?? []).map(native)
                return .int(PaletteSearch.score(query, in: candidates))
            })))
        }
        if let queries = spec["quickOpenQueries"] as? [String] {
            out.append(("quickOpenQueries", .array(queries.map { raw in
                let query = QuickOpenQuery(native(raw))
                return .object([("filter", .string(filterName(query.filter))), ("terms", .string(query.terms))])
            })))
        }
        if let palettes = spec["palettes"] as? [[String: Any]] {
            out.append(("palettes", .array(palettes.map(paletteDump))))
        }
        if let recent = spec["recentStore"] as? [String: Any] {
            out.append(("recentStore", recentStoreDump(recent)))
        }
        if let tours = spec["tour"] as? [[String: Any]] {
            out.append(("tour", .array(tours.map { tourDump($0, relativeTo: input) })))
        }
        if let integration = spec["integration"] as? [String: Any] {
            out.append(("integration", integrationDump(integration)))
        }
        if let store = spec["store"] as? [String: Any] {
            out.append(("store", try storeDump(store)))
        }
        return .object(out)
    }

    /// A native Swift string with the same contents: strings read through
    /// `JSONSerialization` are bridged, and a few `String` operations answer
    /// differently on bridged storage.
    static func native(_ value: String) -> String {
        String(decoding: Array(value.utf8), as: UTF8.self)
    }

    static func hexBytes(_ hex: String) -> [UInt8] {
        var bytes: [UInt8] = []
        var index = hex.startIndex
        while index < hex.endIndex, let next = hex.index(index, offsetBy: 2, limitedBy: hex.endIndex) {
            bytes.append(UInt8(hex[index..<next], radix: 16) ?? 0)
            index = next
        }
        return bytes
    }

    static func lines(_ text: String) -> JSON {
        .array(text.components(separatedBy: "\n").map(JSON.string))
    }

    // MARK: - The command table

    static func table() -> JSON {
        let model = CommandPaletteModel(commands: Command.allCases, bindings: { _ in [] })
        let synonyms = Dictionary(uniqueKeysWithValues: model.entries.map { ($0.command, $0.synonyms) })
        let commands: [JSON] = Command.allCases.map { command in
            .object([
                ("id", .string(command.rawValue)),
                ("title", .string(command.title)),
                ("menu", .string(command.menu.rawValue)),
                ("menuTitle", .string(command.menu.title)),
                ("scopes", .array(CommandScope.allCases.filter { command.scopes.contains($0) }.map { .string($0.rawValue) })),
                ("requires", .string(command.requires.rawValue)),
                ("defaults", .array((KeybindingDefaults.table[command] ?? []).map(bindingDump))),
                ("synonyms", .array((synonyms[command] ?? []).map(JSON.string))),
            ])
        }
        return .object([
            ("commands", .array(commands)),
            ("menus", .array(Command.Menu.allCases.map { .object([("id", .string($0.rawValue)), ("title", .string($0.title))]) })),
            ("scopes", .array(CommandScope.allCases.map { .object([("id", .string($0.rawValue)), ("paletteTitle", .string($0.paletteTitle))]) })),
            ("preconditions", .array(CommandPrecondition.allCases.map { .string($0.rawValue) })),
            ("modifiers", .array(KeyBinding.Modifier.allCases.map { .object([("id", .string($0.rawValue)), ("flag", .int(Int($0.flag.rawValue)))]) })),
            ("kinds", .array(QuickOpenProviderKind.allCases.map { .string($0.rawValue) })),
        ])
    }

    static func context(_ values: [String: Any]) -> CommandContext {
        func flag(_ name: String) -> Bool { values[name] as? Bool ?? false }
        return CommandContext(
            hasDocument: flag("hasDocument"),
            documentHasFile: flag("documentHasFile"),
            hasSelection: flag("hasSelection"),
            canCheckForUpdates: flag("canCheckForUpdates"),
            hasUnsavedChanges: flag("hasUnsavedChanges"),
            hasFindQuery: flag("hasFindQuery"),
            canGoBack: flag("canGoBack"),
            canGoForward: flag("canGoForward"),
            isSpeaking: flag("isSpeaking"),
            caretIsInTable: flag("caretIsInTable"),
            hasChangeMarks: flag("hasChangeMarks"),
            hasQuickLookTarget: flag("hasQuickLookTarget")
        )
    }

    static func contextDump(_ values: [String: Any]) -> JSON {
        let context = context(values)
        return .object([
            ("enabled", .array(Command.allCases.filter { $0.isEnabled(in: context) }.map { .string($0.rawValue) })),
            ("satisfied", .array(CommandPrecondition.allCases.map { .bool($0.isSatisfied(in: context)) })),
        ])
    }

    // MARK: - Key bindings

    static func modifierFlags(_ value: Any?) -> NSEvent.ModifierFlags {
        if let raw = value as? Int { return NSEvent.ModifierFlags(rawValue: UInt(raw)) }
        let names = value as? [String] ?? []
        return names.reduce(into: NSEvent.ModifierFlags()) { flags, name in
            switch name {
            case "command": flags.insert(.command)
            case "shift": flags.insert(.shift)
            case "option": flags.insert(.option)
            case "control": flags.insert(.control)
            case "numericPad": flags.insert(.numericPad)
            case "capsLock": flags.insert(.capsLock)
            case "function": flags.insert(.function)
            case "help": flags.insert(.help)
            default: break
            }
        }
    }

    static func binding(_ value: Any?) -> KeyBinding? {
        guard let object = value as? [String: Any], let key = object["key"] as? String else { return nil }
        return KeyBinding(native(key), modifierFlags(object["modifiers"]))
    }

    static func bindingDump(_ binding: KeyBinding) -> JSON {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let encoded = (try? encoder.encode(binding)).map { String(decoding: $0, as: UTF8.self) }
        return .object([
            ("key", .string(binding.key)),
            ("modifiers", .array(KeyBinding.Modifier.names(in: binding.modifiers).map { .string($0.rawValue) })),
            ("raw", .int(Int(binding.modifiers.rawValue))),
            ("serialized", .string(binding.serialized)),
            ("display", .string(binding.displayString)),
            ("menuKeyEquivalent", .array(binding.menuKeyEquivalent.unicodeScalars.map { .int(Int($0.value)) })),
            ("encoded", .string(encoded)),
            ("reparsed", KeyBinding(parsing: binding.serialized).map { .bool($0 == binding) } ?? .null),
        ])
    }

    static func decodeBindingDump(_ snippet: String) -> JSON {
        do {
            let binding = try JSONDecoder().decode(KeyBinding.self, from: Data(native(snippet).utf8))
            return .object([("binding", bindingDump(binding))])
        } catch {
            return .object([("error", .int((error as NSError).code))])
        }
    }

    static func errorDump(_ error: Error) -> JSON {
        let ns = error as NSError
        return .object([
            ("domain", .string(ns.domain)),
            ("code", .int(ns.code)),
            ("description", .string(error.localizedDescription)),
        ])
    }

    static func overridesDump(_ overrides: [Command: [KeyBinding]]) -> JSON {
        .array(Command.allCases.compactMap { command in
            guard let bindings = overrides[command] else { return nil }
            return .array([.string(command.rawValue), .array(bindings.map { .string($0.serialized) })])
        })
    }

    static func loadDump(_ load: KeybindingLoad) -> JSON {
        switch load {
        case .absent:
            return .object([("state", .string("absent"))])
        case .loaded(let vim, let overrides):
            return .object([
                ("state", .string("loaded")),
                ("vimKeysEnabled", .bool(vim)),
                ("overrides", overridesDump(overrides)),
            ])
        case .unreadable(let error):
            return .object([("state", .string("unreadable")), ("error", errorDump(error))])
        }
    }

    static func encodeStoredDump(_ spec: [String: Any]) -> JSON {
        var overrides: [String: [KeyBinding]] = [:]
        for (name, bindings) in spec["overrides"] as? [String: Any] ?? [:] {
            overrides[native(name)] = (bindings as? [Any] ?? []).compactMap(binding)
        }
        let stored = KeybindingLoad.Stored(vimKeysEnabled: spec["vimKeysEnabled"] as? Bool ?? false, overrides: overrides)
        let pretty = JSONEncoder()
        pretty.outputFormatting = [.prettyPrinted, .sortedKeys]
        let compact = JSONEncoder()
        compact.outputFormatting = [.sortedKeys]
        let prettyData = (try? pretty.encode(stored)) ?? Data()
        let compactData = (try? compact.encode(stored)) ?? Data()
        return .object([
            ("pretty", lines(String(decoding: prettyData, as: UTF8.self))),
            ("compact", .string(String(decoding: compactData, as: UTF8.self))),
            ("roundTrip", loadDump(KeybindingLoad.decode(prettyData))),
        ])
    }

    // MARK: - Fuzzy matching

    static let highlightKey = NSAttributedString.Key("upleft.highlight")

    /// The attribute runs of `highlighted`, as `[location, length, highlighted]`.
    static func highlightRuns(_ haystack: String, positions: [Int]) -> JSON {
        let attributed = FuzzyMatcher.highlighted(
            haystack, positions: positions,
            base: [NSAttributedString.Key("upleft.base"): NSNumber(value: 0)],
            highlight: [highlightKey: NSNumber(value: 1)]
        )
        var runs: [JSON] = []
        var index = 0
        while index < attributed.length {
            var range = NSRange(location: 0, length: 0)
            let value = attributed.attribute(highlightKey, at: index, effectiveRange: &range)
            runs.append(.array([.int(range.location), .int(range.length), .bool(value != nil)]))
            index = NSMaxRange(range)
        }
        return .array(runs)
    }

    static func matchDump(_ match: FuzzyMatcher.Match?) -> JSON {
        guard let match else { return .null }
        return .object([("score", .int(match.score)), ("positions", .array(match.positions.map(JSON.int)))])
    }

    static func fuzzyDump(needle: String, haystack: String) -> JSON {
        let match = FuzzyMatcher.match(needle: needle, in: haystack)
        return .object([
            ("match", matchDump(match)),
            ("highlight", match.map { highlightRuns(haystack, positions: $0.positions) } ?? .null),
        ])
    }

    // MARK: - Palette

    static func filterName(_ filter: QuickOpenFilter) -> String {
        switch filter {
        case .all: return "all"
        case .commands: return "commands"
        case .headings: return "headings"
        case .tasks: return "tasks"
        case .assets: return "assets"
        case .links: return "links"
        case .files: return "files"
        case .symbols: return "symbols"
        }
    }

    static func actionDump(_ action: QuickOpenAction) -> JSON {
        switch action {
        case .command(let command): return .object([("command", .string(command.rawValue))])
        case .select(let range): return .object([("select", .range(range))])
        case .open(let url): return .object([("open", .string(url.path))])
        case .openAt(let url, let range): return .object([("openAt", .array([.string(url.path), .range(range)]))])
        }
    }

    static func resultDump(_ result: QuickOpenResult, terms: String) -> JSON {
        let match = FuzzyMatcher.match(needle: terms, in: result.title)
        return .object([
            ("id", .string(result.id)),
            ("kind", .string(result.kind.rawValue)),
            ("title", .string(result.title)),
            ("subtitle", .string(result.subtitle)),
            ("searchText", .string(result.searchText)),
            ("action", actionDump(result.action)),
            ("score", .int(result.score)),
            ("match", matchDump(match)),
            ("highlight", match.map { highlightRuns(result.title, positions: $0.positions) } ?? .null),
        ])
    }

    static func entryDump(_ entry: CommandPaletteEntry) -> JSON {
        .object([
            ("command", .string(entry.command.rawValue)),
            ("title", .string(entry.title)),
            ("synonyms", .array(entry.synonyms.map(JSON.string))),
            ("binding", .string(entry.binding)),
            ("scopes", .array(entry.scopes.map { .string($0.rawValue) })),
            ("scopeLabel", .string(entry.scopeLabel)),
        ])
    }

    static func commands(_ values: [Any]?) -> [Command] {
        (values as? [String] ?? []).compactMap { Command(rawValue: native($0)) }
    }

    static func model(_ spec: [String: Any]) -> CommandPaletteModel {
        var providers: [any QuickOpenProvider] = []
        if let document = spec["document"] as? String {
            providers.append(CurrentDocumentQuickOpenProvider(document: MarkdownParser.parse(native(document))))
        }
        if let files = spec["recentFiles"] as? [String] {
            providers.append(RecentFilesQuickOpenProvider(files: files.map { URL(fileURLWithPath: native($0)) }))
        }
        if let workspace = spec["workspace"] as? [String: Any] {
            let files = (workspace["files"] as? [String] ?? []).map { URL(fileURLWithPath: native($0)) }
            let symbols: [QuickOpenResult] = (workspace["symbols"] as? [[String: Any]] ?? []).map { symbol in
                let range = NSRange(location: symbol["location"] as? Int ?? 0, length: symbol["length"] as? Int ?? 0)
                return QuickOpenResult(
                    id: native(symbol["id"] as? String ?? ""), kind: .symbol,
                    title: native(symbol["title"] as? String ?? ""),
                    subtitle: native(symbol["subtitle"] as? String ?? ""),
                    searchText: native(symbol["searchText"] as? String ?? ""),
                    action: .openAt(URL(fileURLWithPath: native(symbol["path"] as? String ?? "/")), range),
                    score: symbol["score"] as? Int ?? 0
                )
            }
            providers.append(WorkspaceQuickOpenProvider(files: files, symbols: symbols))
        }
        let recents = commands(spec["recents"] as? [Any])
        if let entries = spec["entries"] as? [[String: Any]] {
            let custom = entries.compactMap { entry -> CommandPaletteEntry? in
                guard let command = Command(rawValue: native(entry["command"] as? String ?? "")) else { return nil }
                return CommandPaletteEntry(
                    command: command,
                    title: native(entry["title"] as? String ?? ""),
                    synonyms: (entry["synonyms"] as? [String] ?? []).map(native),
                    binding: (entry["binding"] as? String).map(native),
                    scopes: (entry["scopes"] as? [String] ?? []).compactMap { CommandScope(rawValue: native($0)) }
                )
            }
            return CommandPaletteModel(entries: custom, recentCommands: recents, providers: providers)
        }
        let commandList: [Command]
        switch spec["commands"] {
        case let list as [String]: commandList = list.compactMap { Command(rawValue: native($0)) }
        case let mode as String where mode == "enabled":
            let context = context(spec["context"] as? [String: Any] ?? [:])
            commandList = Command.allCases.filter { $0.isEnabled(in: context) }
        default: commandList = Command.allCases
        }
        let bindings: (Command) -> [KeyBinding]
        switch spec["bindings"] as? String {
        case "none": bindings = { _ in [] }
        case "store": bindings = { KeybindingStore.shared.bindings(for: $0) }
        default: bindings = { KeybindingDefaults.table[$0] ?? [] }
        }
        return CommandPaletteModel(commands: commandList, bindings: bindings, recentCommands: recents, providers: providers)
    }

    static func selectionDump(_ model: CommandPaletteModel) -> [(String, JSON)] {
        [
            ("selectedIndex", .int(model.selectedIndex)),
            ("selectedEntry", .string(model.selectedEntry?.command.rawValue)),
            ("selectedResult", .string(model.selectedResult?.id)),
        ]
    }

    static func queryDump(_ model: CommandPaletteModel) -> JSON {
        let parsed = QuickOpenQuery(model.query)
        let trimmed = model.query.trimmingCharacters(in: .whitespacesAndNewlines)
        let results: [JSON] = model.results.map { entry in
            .object([
                ("command", .string(entry.command.rawValue)),
                ("score", .int(PaletteSearch.score(trimmed, in: entry.searchCandidates))),
            ])
        }
        return .object([
            ("query", .string(model.query)),
            ("filter", .string(filterName(parsed.filter))),
            ("terms", .string(parsed.terms)),
            ("results", .array(results)),
            ("quick", .array(model.quickResults.map { resultDump($0, terms: parsed.terms) })),
        ] + selectionDump(model))
    }

    static func paletteDump(_ spec: [String: Any]) -> JSON {
        var model = model(spec)
        var out: [(String, JSON)] = []
        if spec["dumpEntries"] as? Bool == true {
            out.append(("entries", .array(model.entries.map(entryDump))))
        }
        out.append(("recents", .array(model.recentCommands.map { .string($0.rawValue) })))
        var queries: [JSON] = []
        for query in spec["queries"] as? [String] ?? [] {
            model.updateQuery(native(query))
            queries.append(queryDump(model))
        }
        out.append(("queries", .array(queries)))
        var steps: [JSON] = []
        for step in spec["steps"] as? [[String: Any]] ?? [] {
            if let query = step["query"] as? String { model.updateQuery(native(query)) }
            if let offset = step["move"] as? Int { model.moveSelection(by: offset) }
            if let index = step["select"] as? Int { model.select(index: index) }
            if let raw = step["record"] as? String, let command = Command(rawValue: native(raw)) { model.record(command) }
            steps.append(.object(selectionDump(model) + [
                ("recents", .array(model.recentCommands.map { .string($0.rawValue) })),
                ("quickCount", .int(model.quickResults.count)),
                ("first", .string(model.quickResults.first?.id)),
            ]))
        }
        out.append(("steps", .array(steps)))
        return .object(out)
    }

    // MARK: - Recent commands

    /// The real home's preferences folder: `UserDefaults` ignores
    /// `CFFIXED_USER_HOME`, so a suite's plist lands there.
    static func realPreferencesFile(suite: String) -> String? {
        guard let entry = getpwuid(getuid()), let home = entry.pointee.pw_dir else { return nil }
        return String(cString: home) + "/Library/Preferences/\(suite).plist"
    }

    static func recentStoreDump(_ spec: [String: Any]) -> JSON {
        let suite = "upleft.conformance.palette.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer {
            defaults.removePersistentDomain(forName: suite)
            if let path = realPreferencesFile(suite: suite) { try? FileManager.default.removeItem(atPath: path) }
        }
        let key = (spec["key"] as? String).map(native) ?? "commandPalette.recentCommands"
        if let initial = spec["initial"] { defaults.set(initial, forKey: key) }
        let store = UserDefaultsCommandPaletteRecentStore(defaults: defaults, key: key, limit: spec["limit"] as? Int ?? 12)
        func stored() -> JSON {
            guard let array = defaults.array(forKey: key) else { return .null }
            return .array(array.map { ($0 as? String).map(JSON.string) ?? .string("<non-string>") })
        }
        var steps: [JSON] = [.object([("recent", .array(store.recentCommands().map { .string($0.rawValue) })), ("stored", stored())])]
        for raw in spec["record"] as? [String] ?? [] {
            guard let command = Command(rawValue: native(raw)) else { continue }
            store.record(command)
            steps.append(.object([("recent", .array(store.recentCommands().map { .string($0.rawValue) })), ("stored", stored())]))
        }
        return .array(steps)
    }

    // MARK: - Welcome tour

    /// `file` is relative to the input's folder (`repositoryRoot` resolves
    /// to the SwiftPM build folder, not the repository).
    static func tourDump(_ spec: [String: Any], relativeTo input: URL) -> JSON {
        let source: String
        if let file = spec["file"] as? String {
            let url = input.deletingLastPathComponent().appendingPathComponent(file)
            source = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
        } else {
            source = native(spec["source"] as? String ?? "")
        }
        let lookup: (Command) -> KeyBinding?
        switch spec["bindings"] {
        case let mode as String where mode == "none": lookup = { _ in nil }
        case let table as [String: Any]:
            var custom: [Command: KeyBinding] = [:]
            for (name, value) in table {
                if let command = Command(rawValue: native(name)), let binding = binding(value) { custom[command] = binding }
            }
            lookup = { custom[$0] }
        default: lookup = { KeybindingDefaults.table[$0]?.first }
        }
        let rendered: JSON
        do {
            rendered = .object([("text", lines(try WelcomeTour.render(source, binding: lookup)))])
        } catch let error as WelcomeTour.Error {
            switch error {
            case .malformedToken(let token): rendered = .object([("malformedToken", .string(token))])
            case .unknownCommand(let name): rendered = .object([("unknownCommand", .string(name))])
            case .missingBinding(let command): rendered = .object([("missingBinding", .string(command.rawValue))])
            }
        } catch {
            rendered = .object([("other", .string("\(error)"))])
        }
        return .object([("tokens", .array(WelcomeTour.tokens(in: source).map(JSON.string))), ("render", rendered)])
    }

    // MARK: - Integrations

    static func integrationDump(_ spec: [String: Any]) -> JSON {
        let accepts: [JSON] = (spec["accepts"] as? [String] ?? []).map { raw in
            let value = native(raw)
            let url = value.contains("://") ? URL(string: value) : URL(fileURLWithPath: value)
            return url.map { .bool(NativeIntegrationPolicy.accepts($0)) } ?? .null
        }
        let normalized: [JSON] = (spec["normalized"] as? [String] ?? []).map { raw in
            .string(NativeIntegrationPolicy.normalizedPath(native(raw))?.path)
        }
        let pluginkit: [JSON] = (spec["pluginkit"] as? [[String: String]] ?? []).map { item in
            .bool(SystemIntegration.isEnabled(
                inPluginKitListing: native(item["listing"] ?? ""), identifier: native(item["identifier"] ?? "")
            ))
        }
        return .object([
            ("accepts", .array(accepts)),
            ("normalized", .array(normalized)),
            ("pluginkit", .array(pluginkit)),
            ("claimedExtensions", .array(SystemIntegration.claimedExtensions.map(JSON.string))),
            ("claimedTypes", .array(SystemIntegration.claimedTypes.map { .string($0.identifier) })),
            ("commandLineNames", .array(SystemIntegration.commandLineNames.map(JSON.string))),
            ("extensionIdentifiers", .array([
                .string(SystemIntegration.previewExtensionIdentifier),
                .string(SystemIntegration.thumbnailExtensionIdentifier),
            ])),
            ("markdownExtensions", .array(NativeIntegrationPolicy.markdownExtensions.sorted().map(JSON.string))),
        ])
    }

    // MARK: - The keybinding store

    /// `(keyCode, characters)` for an event that produces `key`.
    static func eventKey(_ key: String) -> (UInt16, String) {
        switch key {
        case "space": return (49, " ")
        case "left": return (123, "\u{F702}")
        case "right": return (124, "\u{F703}")
        case "down": return (125, "\u{F701}")
        case "up": return (126, "\u{F700}")
        case "return": return (36, "\r")
        case "enter": return (76, "\u{3}")
        case "tab": return (48, "\t")
        case "escape": return (53, "\u{1B}")
        case "delete": return (51, "\u{7F}")
        case "pageup": return (116, "\u{F72C}")
        case "pagedown": return (121, "\u{F72D}")
        default: return (0, key)
        }
    }

    static func event(keyCode: UInt16, characters: String, modifiers: NSEvent.ModifierFlags) -> NSEvent? {
        NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: 0,
            context: nil, characters: characters, charactersIgnoringModifiers: characters,
            isARepeat: false, keyCode: keyCode
        )
    }

    static func resolve(_ event: NSEvent?, in store: KeybindingStore) -> JSON {
        .array(CommandScope.allCases.map { scope in
            .string(event.flatMap { store.command(for: $0, scope: scope)?.rawValue })
        })
    }

    static func fileDump(_ url: URL) -> JSON {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory) else { return .null }
        if isDirectory.boolValue { return .object([("directory", .bool(true))]) }
        guard let data = try? Data(contentsOf: url) else { return .object([("unreadable", .bool(true))]) }
        if let text = String(data: data, encoding: .utf8) { return .object([("lines", lines(text))]) }  // drops a BOM
        return .object([("hex", .string(data.map { String(format: "%02x", $0) }.joined()))])
    }

    static func stateDump(_ store: KeybindingStore, file: URL, events: [[String: Any]]) -> [(String, JSON)] {
        let lookup: [JSON] = Command.allCases.flatMap { command in
            store.bindings(for: command).map { binding -> JSON in
                let (code, characters) = eventKey(binding.key)
                return .array([.string(command.rawValue), .string(binding.serialized),
                               resolve(event(keyCode: code, characters: characters, modifiers: binding.modifiers), in: store)])
            }
        }
        let explicit: [JSON] = events.map { spec in
            resolve(event(
                keyCode: UInt16(spec["keyCode"] as? Int ?? 0),
                characters: native(spec["characters"] as? String ?? ""),
                modifiers: modifierFlags(spec["modifiers"])
            ), in: store)
        }
        return [
            ("loadFailure", store.loadFailure.map(errorDump) ?? .null),
            ("lastPersistenceError", .bool(store.lastPersistenceError != nil)),
            ("vimKeysEnabled", .bool(store.vimKeysEnabled)),
            ("overridden", .array(Command.allCases.filter { store.isOverridden($0) }.map { .string($0.rawValue) })),
            ("bindings", .array(Command.allCases.map { command in
                .array([.string(command.rawValue), .array(store.bindings(for: command).map { .string($0.serialized) })])
            })),
            ("primary", .array(Command.allCases.map { .string(store.primaryBinding(for: $0)?.displayString) })),
            ("lookup", .array(lookup)),
            ("events", .array(explicit)),
            ("file", fileDump(file)),
        ]
    }

    static func storeDump(_ spec: [String: Any]) throws -> JSON {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("upleft-palette-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let support = root.appendingPathComponent("support", isDirectory: true)
        let file = support.appendingPathComponent("keybindings.json")
        if spec["supportIsFile"] as? Bool == true {
            try Data("not a directory".utf8).write(to: support)
        } else if spec["file"] != nil || spec["fileHex"] != nil || spec["fileIsDirectory"] != nil {
            try FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        }
        if let text = spec["file"] as? String { try Data(native(text).utf8).write(to: file) }
        if let hex = spec["fileHex"] as? String { try Data(hexBytes(hex)).write(to: file) }
        if spec["fileIsDirectory"] as? Bool == true {
            try FileManager.default.createDirectory(at: file, withIntermediateDirectories: true)
        }
        setenv("DOWNRIGHT_SUPPORT_DIRECTORY", support.path, 1)

        let store = KeybindingStore.shared
        let events = spec["events"] as? [[String: Any]] ?? []
        var reported: [JSON] = []
        if spec["onLoadFailure"] as? Bool == true {
            store.onLoadFailure = { reported.append(.string($0.localizedDescription)) }
        }
        var states: [JSON] = [.object([("op", .string("load"))] + stateDump(store, file: file, events: events))]
        for operation in spec["operations"] as? [[String: Any]] ?? [] {
            var extra: [(String, JSON)] = []
            if let raw = operation["set"] as? String, let command = Command(rawValue: native(raw)) {
                store.setBinding(binding(operation["binding"]), for: command)
            } else if operation["reset"] as? Bool == true {
                store.resetToDefaults()
            } else if let vim = operation["vim"] as? Bool {
                store.vimKeysEnabled = vim
            } else if let target = binding(operation["conflicts"]),
                      let excluding = Command(rawValue: native(operation["excluding"] as? String ?? "")) {
                extra.append(("conflicts", .array(store.conflicts(for: target, excluding: excluding).map { .string($0.rawValue) })))
            }
            states.append(.object([("op", .string(operationName(operation)))] + extra + stateDump(store, file: file, events: events)))
        }
        return .object([("states", .array(states)), ("reported", .array(reported))])
    }

    static func operationName(_ operation: [String: Any]) -> String {
        if let raw = operation["set"] as? String { return "set \(raw)" }
        if operation["reset"] != nil { return "reset" }
        if let vim = operation["vim"] as? Bool { return "vim \(vim)" }
        if operation["conflicts"] != nil { return "conflicts" }
        return "unknown"
    }
}
