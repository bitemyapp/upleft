import ElkSwift
import Foundation

// elk-bench <iterations> <graph.json>...
// For each graph: converts the JSON to native Swift values once (as
// beautiful-mermaid passes them), then times `ELK().layout(graph:)` — import,
// layout, export — `iterations` times. Prints name, min and median in µs.

func native(_ value: Any) -> Any {
    switch value {
    case let number as NSNumber:
        if CFGetTypeID(number) == CFBooleanGetTypeID() { return number.boolValue }
        return number.doubleValue
    case let string as String: return string
    case let array as [Any]: return array.map(native)
    case let object as [String: Any]:
        var out: [String: Any] = [:]
        for (k, v) in object { out[k] = native(v) }
        return out
    default: return value
    }
}

let args = CommandLine.arguments
let iterations = Int(args[1])!
for path in args.dropFirst(2) {
    let data = try! Data(contentsOf: URL(fileURLWithPath: path))
    let graph = native(try! JSONSerialization.jsonObject(with: data)) as! [String: Any]
    var samples: [Double] = []
    for _ in 0..<iterations {
        let start = DispatchTime.now().uptimeNanoseconds
        let result = try! ELK().layout(graph: graph)
        let end = DispatchTime.now().uptimeNanoseconds
        precondition(result["width"] != nil)
        samples.append(Double(end - start) / 1000)
    }
    samples.sort()
    let name = URL(fileURLWithPath: path).lastPathComponent
    print(String(format: "%@\t%.0f\t%.0f", name, samples[0], samples[samples.count / 2]))
}
