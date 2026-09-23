import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// Swift side of the `html-export` suite: `HTMLExporter.html()` for a
/// document, as the window controller's `exporter(forPrint:)` builds it.
///
///   downright-app-oracle html-export <file.md> <out.json> [--theme NAME] [--dark] [--print]
///
/// The document is read with `DocumentIO.read` and parsed with the default
/// options, as `MarkdownDocument` does. The exporter gets the style sheet's
/// theme (`--theme`, Paper Light by default, under the aqua or dark-aqua
/// appearance), the file name without its extension as the title
/// (`displayName`), the file's folder as `baseDirectory`, and a
/// `NativeFragmentImageProvider` over that style sheet, so math and diagrams
/// are embedded as PNG data URIs. `html()` runs with the appearance current,
/// as it is on the main thread of an app in that appearance, so dynamic
/// colours resolve the same way.
///
/// The output is `{"lines": html.components(separatedBy: "\n")}`, so a
/// difference names its line; embedded images are compared byte for byte
/// inside those lines.
///
/// `upleft-oracle html-export` (crates/conformance/src/dump/html_export.rs)
/// writes the same.
enum HTMLExportDump {
    struct Flags {
        var theme = "Paper Light"
        var dark = false
        var print = false
    }

    static func flags(_ arguments: [String]) throws -> Flags {
        var flags = Flags()
        var iterator = arguments.makeIterator()
        while let flag = iterator.next() {
            switch flag {
            case "--theme":
                guard let name = iterator.next() else { throw AppOracleError(description: "--theme needs a value") }
                flags.theme = name
            case "--dark": flags.dark = true
            case "--print": flags.print = true
            default: throw AppOracleError(description: "unknown flag \(flag)")
            }
        }
        return flags
    }

    static func html(input: URL, flags: Flags) throws -> String {
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == flags.theme }) else {
            throw AppOracleError(description: "unknown theme \(flags.theme)")
        }
        let appearance = NSAppearance(named: flags.dark ? .darkAqua : .aqua)!
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let url = input.standardizedFileURL
        let (text, _) = try DocumentIO.read(contentsOf: url)
        let exporter = HTMLExporter(
            document: MarkdownParser.parse(text),
            theme: styleSheet.theme,
            title: url.deletingPathExtension().lastPathComponent,
            baseDirectory: url.deletingLastPathComponent(),
            imageProvider: NativeFragmentImageProvider(styleSheet: styleSheet),
            forPrint: flags.print
        )
        var html = ""
        appearance.performAsCurrentDrawingAppearance { html = exporter.html() }
        return html
    }

    static func run(input: URL, flags arguments: [String]) throws -> JSON {
        let html = try html(input: input, flags: flags(arguments))
        return .object([("lines", .array(html.components(separatedBy: "\n").map { JSON.string($0) }))])
    }
}
