import CoreFoundation
import ElkSwift
import Foundation

/// `downright-oracle elk <graph.json> <out.json>` — lays out an ELK JSON graph
/// through the same entry point beautiful-mermaid-swift uses
/// (`ELK().layout(graph:)`, default options and timeout) and dumps the result.
/// `crates/conformance/src/dump/elk.rs` emits the same shape from `upleft-elk`.
///
/// The input is converted to the native Swift values beautiful-mermaid passes
/// (`String`, `Double`, `Bool`, `[Any]`, `[String: Any]`), never `NSNumber`,
/// so the importer's dynamic casts behave as they do in Downright.
enum ElkDump {
    /// How many independent processes lay the graph out. elk-swift's output
    /// is not deterministic: `NetworkSimplex.treeEdges` is a `Set<NEdge>`
    /// hashed by object address and iterated in hash order, so the layering
    /// of graphs with several optimal layerings varies from run to run (and
    /// hashing is seeded per process). Each process sees a different order.
    static let samples = 16

    /// `downright-oracle elk`: the distinct outputs of `samples` runs, in
    /// order of first appearance. One output is written as is; several as
    /// `{"alternatives": [...]}`, which `conform` accepts if the Rust output
    /// equals any of them.
    ///
    /// When the instrumented copy of elk-swift is built
    /// (`crates/elk/tools/elklab.sh` → `target/elklab`), its output is one
    /// more candidate: elk-swift with its two nondeterministic choices made
    /// deterministically (NetworkSimplex tree edges in insertion order,
    /// HyperEdgeCycleDetector ties to the first candidate) — the choices
    /// upleft-elk makes. On every graph where elk-swift is deterministic it is
    /// byte-identical to the real output.
    static func sampled(_ input: URL, to output: String) throws {
        var outputs: [String] = []
        let executable = URL(fileURLWithPath: Bundle.main.executablePath ?? CommandLine.arguments[0])
        let scratch = FileManager.default.temporaryDirectory
            .appendingPathComponent("downright-oracle-elk-\(getpid())-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        for sample in 0..<samples {
            let file = scratch.appendingPathComponent("\(sample).json")
            let process = Process()
            process.executableURL = executable
            process.arguments = ["elk-once", input.path, file.path]
            try process.run()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else {
                throw OracleError.usage("elk layout exited with status \(process.terminationStatus) (sample \(sample))")
            }
            let text = try String(contentsOf: file, encoding: .utf8)
            if !outputs.contains(text) { outputs.append(text) }
        }
        // The instrumented copy only arbitrates where elk-swift itself has
        // been seen to disagree with itself. On a graph whose samples all
        // agree, the real output is the only acceptable one, so a drift in
        // the instrumented copy can never loosen the gate.
        if outputs.count > 1, let lab = labExecutable() {
            let file = scratch.appendingPathComponent("lab.json")
            let process = Process()
            process.executableURL = lab
            process.arguments = [input.path, file.path]
            try process.run()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else {
                throw OracleError.usage("elklab exited with status \(process.terminationStatus)")
            }
            let text = try String(contentsOf: file, encoding: .utf8)
            if !outputs.contains(text) { outputs.append(text) }
        }
        let text = outputs.count == 1 ? outputs[0] : "{\"alternatives\":[" + outputs.joined(separator: ",") + "]}"
        try text.write(toFile: output, atomically: true, encoding: .utf8)
    }

    /// `target/elklab/.build/release/lab`, found by walking up from this
    /// executable (which lives under `target/oracle`).
    static func labExecutable() -> URL? {
        var directory = URL(fileURLWithPath: Bundle.main.executablePath ?? CommandLine.arguments[0])
            .resolvingSymlinksInPath().deletingLastPathComponent()
        while directory.path != "/" {
            let candidate = directory.appendingPathComponent("elklab/.build/release/lab")
            if FileManager.default.isExecutableFile(atPath: candidate.path) { return candidate }
            directory = directory.deletingLastPathComponent()
        }
        return nil
    }

    /// `downright-oracle elk-once`: one layout in this process.
    static func layout(_ input: URL) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let graph = native(try JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            throw OracleError.usage("ELK graph must be a JSON object")
        }
        let result = try ELK().layout(graph: graph)
        return node(result)
    }

    /// JSON as beautiful-mermaid builds it: numbers are `Double`.
    static func native(_ value: Any) -> Any {
        switch value {
        case let number as NSNumber:
            if CFGetTypeID(number) == CFBooleanGetTypeID() { return number.boolValue }
            return number.doubleValue
        case let string as String:
            return string
        case let array as [Any]:
            return array.map(native)
        case let object as [String: Any]:
            var out: [String: Any] = [:]
            for (key, value) in object { out[key] = native(value) }
            return out
        default:
            return value
        }
    }

    static func objects(_ value: Any?) -> [[String: Any]] {
        (value as? [[String: Any]]) ?? []
    }

    static func number(_ value: Any?) -> JSON {
        guard let value = value as? Double else { return .null }
        return .double(value)
    }

    static func point(_ value: Any?) -> JSON {
        guard let point = value as? [String: Any] else { return .null }
        return .array([number(point["x"]), number(point["y"])])
    }

    static func node(_ node: [String: Any]) -> JSON {
        .object([
            ("id", .string(node["id"] as? String)),
            ("x", number(node["x"])),
            ("y", number(node["y"])),
            ("width", number(node["width"])),
            ("height", number(node["height"])),
            ("labels", .array(objects(node["labels"]).map(label))),
            ("ports", .array(objects(node["ports"]).map(port))),
            ("children", .array(objects(node["children"]).map(ElkDump.node))),
            ("edges", .array(objects(node["edges"]).map(edge))),
            ("layoutOptions", options(node["layoutOptions"])),
        ])
    }

    static func port(_ port: [String: Any]) -> JSON {
        .object([
            ("id", .string(port["id"] as? String)),
            ("x", number(port["x"])),
            ("y", number(port["y"])),
            ("width", number(port["width"])),
            ("height", number(port["height"])),
            ("labels", .array(objects(port["labels"]).map(label))),
            ("layoutOptions", options(port["layoutOptions"])),
        ])
    }

    static func label(_ label: [String: Any]) -> JSON {
        .object([
            ("id", .string(label["id"] as? String)),
            ("text", .string(label["text"] as? String)),
            ("x", number(label["x"])),
            ("y", number(label["y"])),
            ("width", number(label["width"])),
            ("height", number(label["height"])),
        ])
    }

    static func edge(_ edge: [String: Any]) -> JSON {
        .object([
            ("id", .string(edge["id"] as? String)),
            ("sources", .array(((edge["sources"] as? [String]) ?? []).map { .string($0) })),
            ("targets", .array(((edge["targets"] as? [String]) ?? []).map { .string($0) })),
            ("sections", .array(objects(edge["sections"]).map(section))),
            ("labels", .array(objects(edge["labels"]).map(label))),
            ("layoutOptions", options(edge["layoutOptions"])),
        ])
    }

    static func section(_ section: [String: Any]) -> JSON {
        .object([
            ("id", .string(section["id"] as? String)),
            ("startPoint", point(section["startPoint"])),
            ("endPoint", point(section["endPoint"])),
            ("bendPoints", .array(objects(section["bendPoints"]).map { point($0) })),
        ])
    }

    /// The exporter keeps only `String`, `Double`, `Int`, and `Bool` values;
    /// they are listed sorted by key with their dynamic type.
    static func options(_ value: Any?) -> JSON {
        guard let options = value as? [String: Any] else { return .array([]) }
        return .array(options.keys.sorted().map { key in
            let value = options[key]!
            let typed: JSON
            switch value {
            case let string as String: typed = .array([.string("string"), .string(string)])
            case let double as Double: typed = .array([.string("double"), .double(double)])
            case let int as Int: typed = .array([.string("int"), .int(int)])
            case let bool as Bool: typed = .array([.string("bool"), .bool(bool)])
            default: typed = .string("?")
            }
            return .array([.string(key), typed])
        })
    }
}
