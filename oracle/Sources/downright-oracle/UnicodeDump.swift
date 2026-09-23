import Foundation

/// `unicode`: the Swift runtime's String, Character and CharacterSet answers
/// that Upleft reproduces in `upleft-swift-text`, for every Unicode scalar and
/// for the adversarial strings in `corpus/unicode/strings.txt`.
/// `crates/conformance/src/dump/unicode.rs` emits the same shape.
///
/// Scalar sets are run-length encoded as inclusive `[low, high]` ranges over
/// the scalars in order, a run breaking at any gap (so never across the
/// surrogates).
enum UnicodeDump {
    /// The Character properties, in bit order for the per-Character masks.
    static let propertyNames = [
        "isLetter", "isNumber", "isWhitespace", "isNewline", "isPunctuation", "isSymbol",
        "isUppercase", "isLowercase", "isCased", "isASCII",
    ]

    static func mask(_ c: Character) -> Int {
        let bits = [
            c.isLetter, c.isNumber, c.isWhitespace, c.isNewline, c.isPunctuation, c.isSymbol,
            c.isUppercase, c.isLowercase, c.isCased, c.isASCII,
        ]
        var out = 0
        for (index, bit) in bits.enumerated() where bit { out |= 1 << index }
        return out
    }

    /// The `CharacterSet`s Downright's sources use.
    static let characterSets: [(String, CharacterSet)] = [
        ("whitespaces", .whitespaces),
        ("whitespacesAndNewlines", .whitespacesAndNewlines),
        ("newlines", .newlines),
        ("alphanumerics", .alphanumerics),
        ("charactersIn:<>", CharacterSet(charactersIn: "<>")),
        ("charactersIn:+-.", CharacterSet(charactersIn: "+-.")),
        ("charactersIn:-", CharacterSet(charactersIn: "-")),
        ("charactersIn:|", CharacterSet(charactersIn: "|")),
        ("charactersIn:[]", CharacterSet(charactersIn: "[]")),
        ("charactersIn:/?#\\", CharacterSet(charactersIn: "/?#\\")),
        ("charactersIn:/:\\0", CharacterSet(charactersIn: "/:\0")),
        ("charactersIn:./", CharacterSet(charactersIn: "./")),
        ("charactersIn:#", CharacterSet(charactersIn: "#")),
        ("charactersIn:editorPath", CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/._+-")),
    ]

    struct Runs {
        var out: [JSON] = []
        var start: UInt32?
        var previous: UInt32 = 0

        mutating func add(_ value: UInt32, _ member: Bool) {
            if member {
                if let open = start, previous + 1 != value {
                    out.append(.array([.int(Int(open)), .int(Int(previous))]))
                    start = value
                } else if start == nil {
                    start = value
                }
                previous = value
            } else if let open = start {
                out.append(.array([.int(Int(open)), .int(Int(previous))]))
                start = nil
            }
        }

        mutating func finish() -> JSON {
            if let open = start { out.append(.array([.int(Int(open)), .int(Int(previous))])) }
            start = nil
            return .array(out)
        }
    }

    static func scalars(_ s: String) -> JSON { .array(s.unicodeScalars.map { .int(Int($0.value)) }) }

    static func document(input: String) -> JSON {
        var properties = [Runs](repeating: Runs(), count: propertyNames.count)
        var sets = [Runs](repeating: Runs(), count: characterSets.count)
        var lowercased: [JSON] = []
        var uppercased: [JSON] = []
        var combining: [JSON] = []
        var combiningRun: (low: UInt32, high: UInt32, value: UInt8)?
        var scalarCount = 0

        for value in UInt32(0)...0x10FFFF {
            guard let scalar = Unicode.Scalar(value) else { continue }
            scalarCount += 1
            let character = Character(scalar)
            let bits = mask(character)
            for index in properties.indices { properties[index].add(value, bits & (1 << index) != 0) }
            for (index, set) in characterSets.enumerated() { sets[index].add(value, set.1.contains(scalar)) }

            let string = String(character)
            let lower = string.lowercased()
            if lower.unicodeScalars.count != 1 || lower.unicodeScalars.first! != scalar {
                lowercased.append(.array([.int(Int(value)), scalars(lower)]))
            }
            let upper = string.uppercased()
            if upper.unicodeScalars.count != 1 || upper.unicodeScalars.first! != scalar {
                uppercased.append(.array([.int(Int(value)), scalars(upper)]))
            }

            let ccc = scalar.properties.canonicalCombiningClass.rawValue
            if let run = combiningRun, run.value == ccc, run.high + 1 == value {
                combiningRun!.high = value
            } else {
                if let run = combiningRun, run.value != 0 {
                    combining.append(.array([.int(Int(run.low)), .int(Int(run.high)), .int(Int(run.value))]))
                }
                combiningRun = (value, value, ccc)
            }

        }
        if let run = combiningRun, run.value != 0 {
            combining.append(.array([.int(Int(run.low)), .int(Int(run.high)), .int(Int(run.value))]))
        }

        var propertyPairs: [(String, JSON)] = []
        for (index, name) in propertyNames.enumerated() { propertyPairs.append((name, properties[index].finish())) }
        var setPairs: [(String, JSON)] = []
        for (index, set) in characterSets.enumerated() { setPairs.append((set.0, sets[index].finish())) }

        let (strings, pairCount, triples, comparisons) = parseStrings(input)
        return .object([
            ("scalarCount", .int(scalarCount)),
            ("properties", .object(propertyPairs)),
            ("characterSets", .object(setPairs)),
            ("lowercased", .array(lowercased)),
            ("uppercased", .array(uppercased)),
            ("combiningClass", .array(combining)),
            ("normalization", normalization(triples)),
            ("comparisons", compare(comparisons)),
            ("strings", .array(strings.map(stringFacts))),
            ("pairs", pairs(Array(strings.prefix(pairCount)))),
        ])
    }

    static func scalarString(_ hexes: some Sequence<Substring>) -> String {
        var string = ""
        for hex in hexes { string.unicodeScalars.append(Unicode.Scalar(UInt32(hex, radix: 16)!)!) }
        return string
    }

    /// `S <hex scalars>` strings and `N <scalar> : <NFD> : <NFC>` triples; the
    /// header comment names the pair-set size.
    static func parseStrings(_ input: String) -> ([String], Int, [(String, String, String)], [(String, String)]) {
        var strings: [String] = []
        var triples: [(String, String, String)] = []
        var comparisons: [(String, String)] = []
        var pairCount = 0
        for line in input.split(separator: "\n", omittingEmptySubsequences: true) {
            if line.hasPrefix("#") {
                if let range = line.range(of: "The first "), let end = line.range(of: " strings") {
                    pairCount = Int(line[range.upperBound..<end.lowerBound]) ?? 0
                }
                continue
            }
            if line.hasPrefix("N") {
                let parts = line.dropFirst().split(separator: ":", omittingEmptySubsequences: false)
                triples.append((
                    scalarString(parts[0].split(separator: " ")),
                    scalarString(parts[1].split(separator: " ")),
                    scalarString(parts[2].split(separator: " "))
                ))
                continue
            }
            if line.hasPrefix("C") {
                let parts = line.dropFirst().split(separator: "/", omittingEmptySubsequences: false)
                comparisons.append((scalarString(parts[0].split(separator: " ")), scalarString(parts[1].split(separator: " "))))
                continue
            }
            guard line.hasPrefix("S") else { continue }
            strings.append(scalarString(line.dropFirst().split(separator: " ")))
        }
        return (strings, pairCount, triples, comparisons)
    }

    /// One base-32 digit per comparison pair: bits `a == b` 1, `a < b` 2,
    /// `b < a` 4, `a.hasPrefix(b)` 8, `a.contains(b)` 16.
    static func compare(_ pairs: [(String, String)]) -> JSON {
        let digits = Array("0123456789abcdefghijklmnopqrstuv")
        var out = ""
        for (a, b) in pairs {
            var bits = 0
            if a == b { bits |= 1 }
            if a < b { bits |= 2 }
            if b < a { bits |= 4 }
            if a.hasPrefix(b) { bits |= 8 }
            if a.contains(b) { bits |= 16 }
            out.append(digits[bits])
        }
        return .object([("count", .int(pairs.count)), ("results", .string(out))])
    }

    /// One base-32 digit per triple (scalar, NFD, NFC): bits `s == nfd` 1,
    /// `s == nfc` 2, `nfd == nfc` 4, `s < nfd` 8, `nfd < s` 16.
    static func normalization(_ triples: [(String, String, String)]) -> JSON {
        let digits = Array("0123456789abcdefghijklmnopqrstuv")
        var out = ""
        for (scalar, nfd, nfc) in triples {
            var bits = 0
            if scalar == nfd { bits |= 1 }
            if scalar == nfc { bits |= 2 }
            if nfd == nfc { bits |= 4 }
            if scalar < nfd { bits |= 8 }
            if nfd < scalar { bits |= 16 }
            out.append(digits[bits])
        }
        return .object([("count", .int(triples.count)), ("results", .string(out))])
    }

    static func stringFacts(_ string: String) -> JSON {
        .object([
            ("characters", .array(string.map { .int($0.unicodeScalars.count) })),
            ("count", .int(string.count)),
            ("masks", .array(string.map { .int(mask($0)) })),
            ("lowercased", .string(string.lowercased())),
            ("uppercased", .string(string.uppercased())),
            ("trimmedWhitespaces", .string(string.trimmingCharacters(in: .whitespaces))),
            ("trimmedWhitespacesAndNewlines", .string(string.trimmingCharacters(in: .whitespacesAndNewlines))),
            ("reversedCharacters", .array(string.reversed().map { .int($0.unicodeScalars.count) })),
        ])
    }

    /// One row per string: a base-32 digit per partner, bits
    /// `==` 1, `<` 2, `hasPrefix` 4, `hasSuffix` 8, `contains` 16.
    static func pairs(_ strings: [String]) -> JSON {
        let digits = Array("0123456789abcdefghijklmnopqrstuv")
        return .array(strings.map { a in
            var row = ""
            for b in strings {
                var bits = 0
                if a == b { bits |= 1 }
                if a < b { bits |= 2 }
                if a.hasPrefix(b) { bits |= 4 }
                if a.hasSuffix(b) { bits |= 8 }
                if a.contains(b) { bits |= 16 }
                row.append(digits[bits])
            }
            return .string(row)
        })
    }
}
