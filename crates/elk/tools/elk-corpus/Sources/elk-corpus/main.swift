import BeautifulMermaid
import Foundation

// elk-corpus <sources.json> <out-dir>
//
// sources.json: [{"name": "...", "source": "<mermaid>"}]. For each diagram,
// runs `GraphLayout(config: LayoutConfig()).layout(MermaidParser.parse(source))`
// — the layout Downright's `MermaidImageRenderer(config: LayoutConfig())`
// performs — and writes every ELK graph handed to `elkLayoutSync` as
// `<out-dir>/<name>.json` (`<name>.<n>.json` when there are several, e.g.
// the flat fallback after a failed layout).

var captured: [[String: Any]] = []

@_dynamicReplacement(for: elkLayoutSync(_:))
public func capturingElkLayoutSync(_ graph: ElkNode) throws -> ElkNode {
    captured.append(graph)
    return try elkLayoutSync(graph)
}

/// Swift values → JSON. Doubles print shortest-round-trip; integral doubles
/// keep a `.0` so they read back as doubles.
func json(_ value: Any) -> String {
    switch value {
    case let s as String:
        let data = try! JSONSerialization.data(withJSONObject: [s], options: [.withoutEscapingSlashes])
        let text = String(data: data, encoding: .utf8)!
        return String(text.dropFirst().dropLast())
    case let b as Bool:
        return b ? "true" : "false"
    case let d as Double:
        return "\(d)"
    case let f as CGFloat:
        return "\(Double(f))"
    case let i as Int:
        return "\(Double(i))"
    case let a as [Any]:
        return "[" + a.map(json).joined(separator: ",") + "]"
    case let o as [String: Any]:
        return "{" + o.keys.sorted().map { json($0) + ":" + json(o[$0]!) }.joined(separator: ",") + "}"
    default:
        fatalError("unsupported value \(type(of: value))")
    }
}

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write("usage: elk-corpus <sources.json> <out-dir>\n".data(using: .utf8)!)
    exit(64)
}
let list = try! JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: args[1]))) as! [[String: Any]]
let outDir = URL(fileURLWithPath: args[2])
try! FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
var written = 0
for entry in list {
    let name = entry["name"] as! String
    let source = entry["source"] as! String
    captured = []
    do {
        let graph = try MermaidParser.parse(source)
        _ = try GraphLayout(config: LayoutConfig()).layout(graph)
    } catch {
        FileHandle.standardError.write("\(name): \(error)\n".data(using: .utf8)!)
    }
    for (index, graph) in captured.enumerated() {
        let file = captured.count == 1 ? "\(name).json" : "\(name).\(index).json"
        try! (json(graph) + "\n").write(to: outDir.appendingPathComponent(file), atomically: true, encoding: .utf8)
        written += 1
    }
}
print("wrote \(written) graphs")
