// swift-tools-version:5.9
import PackageDescription

// Times elk-swift (the exact revision Downright resolves) laying out ELK JSON
// graphs, for comparison with `cargo run --release -p upleft-elk --example elk_bench`.
let package = Package(
    name: "ElkBench",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/lukilabs/elk-swift", exact: "1.0.2"),
    ],
    targets: [
        .executableTarget(name: "elk-bench", dependencies: [.product(name: "ElkSwift", package: "elk-swift")]),
    ]
)
