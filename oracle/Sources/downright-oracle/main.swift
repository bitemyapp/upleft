import AppKit
import MarkdownCore
import MarkdownRender

// downright-oracle — the Swift reference for Upleft's conformance harness.
//
//   downright-oracle parse    <file.md> <out.json>
//   downright-oracle markup   <file.md> <out.json>
//   downright-oracle decorate <file.md> <out.json> [--mode M] [--theme NAME] [--dark]
//   downright-oracle incremental <file.md> <out.json> [--mode M] [--theme NAME] [--dark]
//   downright-oracle displaymap  <file.md> <out.json> [--mode M] [--theme NAME] [--dark]
//   downright-oracle clipboard   <file.md> <out.json>
//   downright-oracle render   <file.md> <out.png> [--layout out.json] [--mode M]
//                             [--theme NAME] [--dark] [--width W] [--height H]
//   downright-oracle stylesheet   <file.md> <out.json> [--theme NAME] [--dark]
//   downright-oracle highlight    <file.md> <out.json>
//   downright-oracle vscode-theme <theme.json> <out.json>
//   downright-oracle mermaid-parse  <file.mmd> <out.json>
//   downright-oracle mermaid-layout <file.mmd> <out.json> [--theme NAME] [--dark]
//   downright-oracle mermaid        <file.mmd> <out.png>  [--theme NAME] [--dark]
//   downright-oracle math         <file.tex> <out.png>  [--theme NAME] [--dark]
//   downright-oracle math-tree    <file.tex> <out.json> [--theme NAME] [--dark]
//   downright-oracle bench-math   <dir> <out.json>
//   downright-oracle density-model <file.md> <out.json> [--theme NAME] [--dark]
//   downright-oracle density-hover <file.md> <out.png>  [--density leading|trailing]
//                             [--hover 0.25|0.5|0.75|outline] [render flags]
//   (`render` also takes `--density leading|trailing`.)
//
// `upleft-oracle` (crates/conformance) takes identical arguments and writes
// identical formats.

func usage() -> Never {
    FileHandle.standardError.write("""
    usage:
      downright-oracle parse    <file.md> <out.json>
      downright-oracle markup   <file.md> <out.json>
      downright-oracle decorate <file.md> <out.json> [--mode read|live|source] [--theme NAME] [--dark]
      downright-oracle render   <file.md> <out.png> [--layout out.json] [--mode M] [--theme NAME] [--dark] [--width W] [--height H] [--capture headless|view|screen]

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
    var captureFromScreen = false
    var headless = true
    /// `--density leading|trailing`: attach the density gutter as the app does.
    var density: String?
    /// `--hover`: which state `density-hover` drives the gutter into.
    var hover: String?

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
            case "--density":
                density = required(iterator.next())
                guard density == "leading" || density == "trailing" else { usage() }
            case "--hover": hover = required(iterator.next())
            case "--capture":
                switch required(iterator.next()) {
                case "screen": captureFromScreen = true; headless = false
                case "view": captureFromScreen = false; headless = false
                case "headless": captureFromScreen = false; headless = true
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

    case "unicode":
        try write(UnicodeDump.document(input: String(contentsOf: input, encoding: .utf8)), to: output)

    case "core-text":
        try write(CoreTextDump.document(data: try Data(contentsOf: input), url: input), to: output)

    case "bench-core-text":
        try write(CoreTextBench.run(text: try String(contentsOf: input, encoding: .utf8)), to: output)

    case "markup":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(MarkupDump.document(text), to: output)

    case "markup-bench":
        try MarkupBench.run(input, output: output)

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

    case "incremental":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(IncrementalDump.run(text: text, flags: flags), to: output)

    case "clipboard":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(ClipboardDump.run(text: text), to: output)

    case "displaymap":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(DisplayMapDump.run(text: text, flags: flags), to: output)

    case "elk":
        try ElkDump.sampled(input, to: output)

    case "elk-once":
        try write(ElkDump.layout(input), to: output)

    case "math":
        try MathDump.image(input, to: output, theme: flags.theme, dark: flags.dark)

    case "math-tree":
        try MathDump.tree(input, to: output, theme: flags.theme, dark: flags.dark)

    case "bench-math":
        try MathBench.run(input, to: output)

    case "render", "probe":
        let request = RenderRequest(
            input: input,
            outputPNG: URL(fileURLWithPath: output),
            outputLayout: flags.layout,
            mode: flags.mode,
            themeName: flags.theme,
            dark: flags.dark,
            width: flags.width,
            height: flags.height,
            captureFromScreen: flags.captureFromScreen,
            headless: flags.headless
        )
        CaptureSession.run(
            request: request,
            scene: command == "render" ? MarkdownScene(density: flags.density) : ProbeScene()
        )

    case "density-model":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(MainActor.assumeIsolated { try DensityModelDump.run(text: text, flags: flags) }, to: output)

    case "bench-density":
        let text = try String(contentsOf: input, encoding: .utf8)
        try write(MainActor.assumeIsolated { try DensityBench.run(text: text, flags: flags) }, to: output)

    case "density-hover":
        let request = RenderRequest(
            input: input, outputPNG: URL(fileURLWithPath: output), outputLayout: nil, mode: flags.mode,
            themeName: flags.theme, dark: flags.dark, width: flags.width, height: flags.height,
            captureFromScreen: flags.captureFromScreen,
            headless: flags.headless
        )
        CaptureSession.run(
            request: request,
            scene: DensityHoverScene(
                markdown: MarkdownScene(density: flags.density ?? "leading"),
                action: flags.hover ?? "0.5"
            )
        )

    case "bench-view":
        let request = RenderRequest(
            input: input, outputPNG: URL(fileURLWithPath: output), outputLayout: nil, mode: flags.mode,
            themeName: flags.theme, dark: flags.dark, width: flags.width, height: flags.height
        )
        ViewBench.run(request: request, output: output)

    case "mermaid-parse":
        try MermaidDump.parse(input, to: output)

    case "mermaid-layout":
        try MermaidDump.layout(input, to: output, themeName: flags.theme, dark: flags.dark)

    case "mermaid":
        try MermaidDump.image(input, to: output, themeName: flags.theme, dark: flags.dark)

    case "mermaid-bench":
        try MermaidBench.run(input, output: output)

    case "mermaid-replay":
        // A `.elkrec` is Swift's own record of one layout (see
        // crates/mermaid/tools/elk-capture): it is the expected output.
        try Data(contentsOf: input).write(to: URL(fileURLWithPath: output))

    case "stylesheet":
        try write(StyleSheetDump.dump(themeName: flags.theme, dark: flags.dark), to: output)

    case "highlight":
        try write(HighlightDump.document(String(contentsOf: input, encoding: .utf8)), to: output)

    case "vscode-theme":
        try write(VSCodeThemeDump.dump(Data(contentsOf: input), url: input), to: output)

    default:
        usage()
    }
} catch {
    FileHandle.standardError.write("\(error)\n".data(using: .utf8)!)
    exit(1)
}
