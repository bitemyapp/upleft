// swift-tools-version: 6.0
import PackageDescription

// The one sanctioned piece of Swift in Upleft (see crates/app/PORTING.md).
//
// FoundationModels and AppIntents have no Objective-C or C interface: their
// types are Swift structs, generics and macros. This package holds only what
// cannot be written in Rust — `@_cdecl` entry points around
// `SystemLanguageModel` / `LanguageModelSession`, and the `OpenMarkdownIntent`
// / `DownrightShortcuts` declarations copied from Downright's AppIntents.swift.
// Everything else in LocalAI.swift and AppIntents.swift is Rust
// (`ai::local_ai`, `integrations::app_intents`).
//
// `crates/app/build.rs` builds this as a static library with
// `swift build -c release` and links it into `upleft-app`. Deployment target
// and language mode match Downright's Package.swift.
let package = Package(
    name: "UpleftSwiftShim",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "UpleftSwiftShim", type: .static, targets: ["UpleftSwiftShim"]),
    ],
    targets: [
        .target(
            name: "UpleftSwiftShim",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
