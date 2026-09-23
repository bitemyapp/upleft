import Foundation
import ElkSwift

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
func pt(_ v: Any?) -> String { let d = v as! [String: Any]; return "(\(d["x"]!),\(d["y"]!))" }
func dump(_ n: [String: Any]) -> String {
    var s = "\(n["id"] ?? "") \(n["x"] ?? "") \(n["y"] ?? "") \(n["width"] ?? "") \(n["height"] ?? "")\n"
    for c in (n["children"] as? [[String: Any]]) ?? [] { s += dump(c) }
    for e in (n["edges"] as? [[String: Any]]) ?? [] {
        for sec in (e["sections"] as? [[String: Any]]) ?? [] {
            s += "\(e["id"] ?? "") \(pt(sec["startPoint"])) \(pt(sec["endPoint"])) \(((sec["bendPoints"] as? [Any]) ?? []).map(pt))\n"
        }
        for l in (e["labels"] as? [[String: Any]]) ?? [] { s += "  label \(l["x"] ?? "") \(l["y"] ?? "")\n" }
    }
    return s
}
let args = CommandLine.arguments
let data = try! Data(contentsOf: URL(fileURLWithPath: args[1]))
let graph = native(try! JSONSerialization.jsonObject(with: data)) as! [String: Any]
let result = try! ELK().layout(graph: graph)
print(dump(result), terminator: "")
