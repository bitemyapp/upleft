// swift-tools-version:5.9
import PackageDescription

// Records what beautiful-mermaid-swift hands ELK and gets back, by running
// its real layout path with `elkLayoutSync` dynamically replaced (build with
// `-enable-implicit-dynamic`, see ../elk-capture.sh). Pinned to the revisions
// Downright resolves. JSON.swift and MermaidModelDump.swift are symlinks to
// the oracle's, so the recorded positioned graphs have the oracle's shape.
let package = Package(
    name: "ElkCapture",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/lukilabs/beautiful-mermaid-swift.git", exact: "1.0.4"),
        .package(url: "https://github.com/lukilabs/elk-swift", exact: "1.0.2"),
    ],
    targets: [
        .executableTarget(
            name: "elk-capture",
            dependencies: [.product(name: "BeautifulMermaid", package: "beautiful-mermaid-swift")]
        ),
    ]
)
