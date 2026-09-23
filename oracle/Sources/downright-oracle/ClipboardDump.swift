import Foundation
@testable import MarkdownRender

/// `clipboard`: `ClipboardSemanticHTML.render(markdown:)` of the document.
enum ClipboardDump {
    static func run(text: String) -> JSON {
        .object([("html", .string(ClipboardSemanticHTML.render(markdown: text)))])
    }
}
