// swift-tools-version: 6.0
import PackageDescription

// The Swift side of Upleft's app-layer conformance suites (html-export,
// spotlight, down-cli, workspace, find, palette, formats, updater, the
// window and panel suites, and the Quick Look suites).
//
// Downright's app is an executable target, which another package cannot
// import, and a second target named `DownrightApp` may not sit in the same
// package graph as the submodule's. So this package does not depend on the
// submodule's package: each module directory under `Sources/` is a symlink to
// the submodule's own sources, compiled here unchanged, with the same module
// names and the same settings as vendor/downright/Package.swift (only the
// app's top-level `main.swift` is left out). The dumps in
// `downright-app-oracle` `@testable import DownrightApp` the way Downright's
// own `DownrightAppTests` do. Nothing here reimplements Downright behaviour.
//
// It is a separate package from `oracle/` so that the core oracle does not
// have to build the whole app (and fetch Sparkle) to run the other suites.
let package = Package(
    name: "DownrightAppOracle",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/apple/swift-markdown.git", revision: "27b7fc1a19068bcea3d2072db0ce86360d1400ed"),
        .package(path: "../../target/rebranded/downright/Vendor/SwiftMath"),
        .package(url: "https://github.com/lukilabs/beautiful-mermaid-swift.git", exact: "1.0.4"),
        .package(url: "https://github.com/sparkle-project/Sparkle.git", exact: "2.9.6"),
    ],
    targets: [
        .target(
            name: "MarkdownCore",
            dependencies: [.product(name: "Markdown", package: "swift-markdown")],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "MarkdownRender",
            dependencies: [
                "MarkdownCore",
                .product(name: "SwiftMath", package: "SwiftMath"),
                .product(name: "BeautifulMermaid", package: "beautiful-mermaid-swift"),
            ],
            resources: [.copy("Themes")],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "DownrightSpotlightMetadata",
            dependencies: ["MarkdownCore"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "drdownright",
            dependencies: ["MarkdownCore"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "DownrightApp",
            dependencies: [
                "MarkdownCore",
                "MarkdownRender",
                "DownrightSpotlightMetadata",
                "drdownright",
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            exclude: ["main.swift"],
            // `-enable-private-imports` lets LocalAIDump reach the Apple
            // adapter's `private` prompt and result helpers through
            // `@_private(sourceFile:) import`; like `-enable-testing`, it
            // changes symbol visibility only.
            swiftSettings: [.swiftLanguageMode(.v5), .unsafeFlags(["-enable-private-imports"])]
        ),
        // The Quick Look extensions' sources (Downright builds them as library
        // targets and links them into .appex executables with a generated
        // `main.swift`). `-enable-private-imports` lets the quicklook-preview
        // scene read the controller's private state and retire its memory
        // watch, as the Rust harness does through test hooks.
        .target(
            name: "DownrightQL",
            dependencies: ["MarkdownCore", "MarkdownRender"],
            swiftSettings: [.swiftLanguageMode(.v5), .unsafeFlags(["-enable-private-imports"])]
        ),
        .target(
            name: "DownrightThumb",
            dependencies: ["MarkdownCore", "MarkdownRender"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .executableTarget(
            name: "downright-app-oracle",
            dependencies: [
                "DownrightApp",
                "MarkdownCore",
                "MarkdownRender",
                "DownrightSpotlightMetadata",
                "drdownright",
                "DownrightQL",
                "DownrightThumb",
            ],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
