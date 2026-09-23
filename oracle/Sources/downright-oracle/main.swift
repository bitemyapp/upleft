import AppKit
import MarkdownCore
import MarkdownRender

// downright-oracle — the Swift reference for Upleft's conformance harness.
//
//   downright-oracle parse    <file.md> <out.json>
//   downright-oracle decorate <file.md> <out.json> [--mode M] [--theme NAME] [--dark]
//   downright-oracle render   <file.md> <out.png> [--layout out.json] [--mode M]
//                             [--theme NAME] [--dark] [--width W] [--height H]
//
// `upleft-oracle` (crates/conformance) takes identical arguments and writes
// identical formats.

func usage() -> Never {
    FileHandle.standardError.write("""
    usage:
      downright-oracle parse    <file.md> <out.json>
      downright-oracle decorate <file.md> <out.json> [--mode read|live|source] [--theme NAME] [--dark]
      downright-oracle render   <file.md> <out.png> [--layout out.json] [--mode M] [--theme NAME] [--dark] [--width W] [--height H] [--capture screen|view]

    """.data(using: .utf8)!)
    exit(64)
}

func required<T>(_ value: T?) -> T {
    guard let value else { usage() }
    return value
}

struct Flags {
    var mode: RenderMode = .live
    var theme = "Paper Light"
    var dark = false
    var width: CGFloat = 1000
    var height: CGFloat = 1400
    var layout: URL?
    var captureFromScreen = true

    init(_ arguments: ArraySlice<String>) {
        var iterator = arguments.makeIterator()
        while let flag = iterator.next() {
            switch flag {
            case "--mode": mode = required(iterator.next().flatMap(RenderMode.init(rawValue:)))
            case "--theme": theme = required(iterator.next())
            case "--dark": dark = true
            case "--width": width = CGFloat(required(iterator.next().flatMap(Double.init)))
            case "--height": height = CGFloat(required(iterator.next().flatMap(Double.init)))
            case "--layout": layout = URL(fileURLWithPath: required(iterator.next()))
            case "--capture":
                switch required(iterator.next()) {
                case "screen": captureFromScreen = true
                case "view": captureFromScreen = false
                default: usage()
                }
            default: usage()
            }
        }
    }
}

func write(_ json: JSON, to path: String) throws {
    try json.text.write(toFile: path, atomically: true, encoding: .utf8)
}

let arguments = CommandLine.arguments
guard arguments.count >= 4 else { usage() }
let command = arguments[1]
let input = URL(fileURLWithPath: arguments[2])
let output = arguments[3]
let flags = Flags(arguments.dropFirst(4))

do {
    switch command {
    case "parse":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(ParseDump.document(MarkdownParser.parse(text)), to: output)

    case "decorate":
        let text = try String(contentsOf: input, encoding: .utf8)
        let appearance = NSAppearance(named: flags.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == flags.theme }) else {
            throw OracleError.unknownTheme(flags.theme, ThemeStore.shared.themes.map(\.name))
        }
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let engine = DecorationEngine(styleSheet: styleSheet)
        engine.policy = flags.mode.policy
        let storage = NSTextStorage(string: text)
        engine.decorate(storage, document: MarkdownParser.parse(text), dirty: .wholesale)
        try write(AttributeDump.storage(storage), to: output)

    case "render":
        let request = RenderRequest(
            input: input,
            outputPNG: URL(fileURLWithPath: output),
            outputLayout: flags.layout,
            mode: flags.mode,
            themeName: flags.theme,
            dark: flags.dark,
            width: flags.width,
            height: flags.height,
            captureFromScreen: flags.captureFromScreen
        )
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let session = RenderSession(request: request)
        app.delegate = session
        app.run()

    default:
        usage()
    }
} catch {
    FileHandle.standardError.write("\(error)\n".data(using: .utf8)!)
    exit(1)
}
