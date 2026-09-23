// swift-tools-version: 6.0
import PackageDescription

// The Swift side of Upleft's conformance harness. It links Downright's own
// MarkdownCore and MarkdownRender from the `vendor/downright` submodule and
// emits the same dumps and captures as `upleft-oracle`; the conformance runner
// compares the two. Nothing here reimplements Downright behaviour.
let package = Package(
    name: "DownrightOracle",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(name: "downright", path: "../vendor/downright"),
        .package(url: "https://github.com/apple/swift-markdown.git", revision: "27b7fc1a19068bcea3d2072db0ce86360d1400ed"),
        .package(url: "https://github.com/lukilabs/beautiful-mermaid-swift.git", exact: "1.0.4"),
    ],
    targets: [
        .executableTarget(
            name: "downright-oracle",
            dependencies: [
                .product(name: "MarkdownCore", package: "downright"),
                .product(name: "MarkdownRender", package: "downright"),
                .product(name: "Markdown", package: "swift-markdown"),
                .product(name: "BeautifulMermaid", package: "beautiful-mermaid-swift"),
            ],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
