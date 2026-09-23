import Foundation

/// A minimal ordered JSON value. Dumps are compared structurally by the
/// conformance runner, so formatting is irrelevant, but key order is kept
/// stable to make a textual diff readable.
indirect enum JSON {
    case null
    case bool(Bool)
    case int(Int)
    /// 64-bit hashes are emitted as fixed-width hex strings so no JSON reader
    /// can round them through a double.
    case hex(UInt64)
    case double(Double)
    case string(String)
    case array([JSON])
    case object([(String, JSON)])

    static func range(_ range: NSRange) -> JSON {
        .array([.int(range.location), .int(range.length)])
    }

    static func range(_ range: NSRange?) -> JSON {
        range.map { JSON.range($0) } ?? .null
    }

    static func string(_ value: String?) -> JSON {
        value.map { JSON.string($0) } ?? .null
    }

    static func int(_ value: Int?) -> JSON {
        value.map { JSON.int($0) } ?? .null
    }

    func serialized(into out: inout String) {
        switch self {
        case .null: out += "null"
        case .bool(let value): out += value ? "true" : "false"
        case .int(let value): out += String(value)
        case .hex(let value): out += "\"" + String(format: "%016llx", value) + "\""
        case .double(let value):
            if value.isNaN { out += "\"nan\"" }
            else if value.isInfinite { out += value < 0 ? "\"-inf\"" : "\"inf\"" }
            // Swift's description is the shortest string that round-trips.
            else { out += "\(value)" }
        case .string(let value): JSON.escape(value, into: &out)
        case .array(let values):
            out += "["
            for (index, value) in values.enumerated() {
                if index > 0 { out += "," }
                value.serialized(into: &out)
            }
            out += "]"
        case .object(let pairs):
            out += "{"
            for (index, (key, value)) in pairs.enumerated() {
                if index > 0 { out += "," }
                JSON.escape(key, into: &out)
                out += ":"
                value.serialized(into: &out)
            }
            out += "}"
        }
    }

    var text: String {
        var out = ""
        serialized(into: &out)
        return out
    }

    /// Escapes by UTF-16 code unit so lone surrogates in a document survive
    /// as `\uXXXX` rather than being replaced.
    static func escape(_ value: String, into out: inout String) {
        out += "\""
        for unit in value.utf16 {
            switch unit {
            case 0x22: out += "\\\""
            case 0x5C: out += "\\\\"
            case 0x0A: out += "\\n"
            case 0x0D: out += "\\r"
            case 0x09: out += "\\t"
            case 0x00..<0x20, 0x7F, 0xD800...0xDFFF:
                out += String(format: "\\u%04x", unit)
            default:
                out += String(utf16CodeUnits: [unit], count: 1)
            }
        }
        out += "\""
    }
}
