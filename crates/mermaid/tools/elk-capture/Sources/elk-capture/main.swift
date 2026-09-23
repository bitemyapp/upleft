import BeautifulMermaid
import Foundation

// elk-capture <corpus-dir> <out-dir>
//
// For every `.mmd` under corpus-dir whose layout goes through ELK, runs
// `GraphLayout(config: LayoutConfig()).layout(MermaidParser.parse(trimmed))`
// — the layout `MermaidRendererBridge` performs — and writes
// `<out-dir>/<stem>.elkrec`:
//
//   {"source": <trimmed source>,
//    "calls": [{"input": <graph>, "output": <laid-out graph> | null}, ...],
//    "positioned": <MermaidModelDump.positionedGraph> | "layoutError": ...}
//
// Every ELK call and its result come from one run, so the record is
// consistent even for the graphs on which elk-swift is nondeterministic.
// upleft-mermaid replays the calls in place of an ELK engine to check
// everything around ELK: the input it builds, and what it makes of the output.

var calls: [(input: [String: Any], output: [String: Any]?)] = []

@_dynamicReplacement(for: elkLayoutSync(_:))
public func recordingElkLayoutSync(_ graph: ElkNode) throws -> ElkNode {
    do {
        let output = try elkLayoutSync(graph)
        calls.append((graph, output))
        return output
    } catch {
        calls.append((graph, nil))
        throw error
    }
}

/// Swift values → JSON. Doubles keep a `.0` when integral so they read back
/// as doubles; `Int`s stay integers.
func anyJSON(_ value: Any) -> JSON {
    switch value {
    case let s as String: return .string(s)
    case let b as Bool: return .bool(b)
    case let d as Double: return .double(d)
    case let f as CGFloat: return .double(Double(f))
    case let i as Int: return .int(i)
    case let a as [Any]: return .array(a.map(anyJSON))
    case let o as [String: Any]: return .object(o.keys.sorted().map { ($0, anyJSON(o[$0]!)) })
    default: fatalError("unsupported value \(type(of: value))")
    }
}

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write("usage: elk-capture <corpus-dir> <out-dir>\n".data(using: .utf8)!)
    exit(64)
}
let corpus = URL(fileURLWithPath: args[1])
let outDir = URL(fileURLWithPath: args[2])
try! FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
let files = try! FileManager.default.contentsOfDirectory(at: corpus, includingPropertiesForKeys: nil)
    .filter { $0.pathExtension == "mmd" }
    .sorted { $0.lastPathComponent < $1.lastPathComponent }

var written = 0
for file in files {
    let source = try! String(contentsOf: file, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
    calls = []
    var pairs: [(String, JSON)] = []
    do {
        let graph = try MermaidParser.parse(source)
        do {
            let positioned = try GraphLayout(config: LayoutConfig()).layout(graph)
            pairs.append(("positioned", MermaidModelDump.positionedGraph(positioned)))
        } catch {
            pairs.append(("layoutError", MermaidModelDump.errorJSON(error)))
        }
    } catch {
        continue
    }
    guard !calls.isEmpty else { continue }
    let recorded: [JSON] = calls.map { call in
        .object([("input", anyJSON(call.input)), ("output", call.output.map(anyJSON) ?? .null)])
    }
    pairs.insert(("calls", .array(recorded)), at: 0)
    pairs.insert(("source", .string(source)), at: 0)
    let name = file.deletingPathExtension().lastPathComponent + ".elkrec"
    try! (JSON.object(pairs).text + "\n").write(to: outDir.appendingPathComponent(name), atomically: true, encoding: .utf8)
    written += 1
}
print("wrote \(written) records")
