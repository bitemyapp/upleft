// swift-tools-version:5.9
import PackageDescription

// Captures the ELK graphs beautiful-mermaid-swift lays out, by running its real
// layout path with `elkLayoutSync` dynamically replaced (build with
// `-Xswiftc -enable-implicit-dynamic`, see ../elk-corpus.sh). Pinned to the
// revisions Downright resolves.
let package = Package(
    name: "ElkCorpus",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/lukilabs/beautiful-mermaid-swift.git", exact: "1.0.4"),
        .package(url: "https://github.com/lukilabs/elk-swift", exact: "1.0.2"),
    ],
    targets: [
        .executableTarget(
            name: "elk-corpus",
            dependencies: [.product(name: "BeautifulMermaid", package: "beautiful-mermaid-swift")]
        ),
    ]
)
