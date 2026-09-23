# Swift

```swift
import Foundation

/// A doc comment with `code` and a URL: https://example.com
@MainActor
public final class Cache<Key: Hashable, Value>: NSObject, @unchecked Sendable {
    private var entries: [Key: Value] = [:]
    static let MAX_ENTRIES = 1_024
    let ratio = 0x1.8p3 + 1e-3 - .5 * 0b1010 / 0o17
    let range = 1..<5, closed = 1...3

    init(capacity: Int = 16) { super.init() }

    func value(for key: Key) async throws -> Value? {
        #if DEBUG
        print("lookup \(key) in \"entries\"")
        #endif
        let raw = #"a "raw" string with \n kept"#
        let deeper = ##"one "# inside"##
        let block = """
            multi-line "quotes" and \(interpolation)
            """
        let rawBlock = #"""
            raw block """ still open
            """#
        /* a /* nested */ block comment */
        guard let hit = entries[key] else { return nil }
        return hit as? Value ?? nil
    }

    @objc dynamic var isEmpty: Bool { entries.isEmpty && self !== nil }
}

let café = "naïve ☕️ 😀"; let π = 3.14159
let unterminated = "runs to the end of the line
let next = true
```

```Swift
struct Point { var x, y: Double }
extension Point: Equatable where Self: Sendable {}
let p = Point(x: 1, y: -2.5e10)
```

```swift
/* an unterminated comment runs to the end
let x = 1
```
