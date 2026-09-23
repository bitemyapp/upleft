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
    ],
    targets: [
        .executableTarget(
            name: "downright-oracle",
            dependencies: [
                .product(name: "MarkdownCore", package: "downright"),
                .product(name: "MarkdownRender", package: "downright"),
            ],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
