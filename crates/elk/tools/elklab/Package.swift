// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "ElkLab",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "ElkSwift"),
        .executableTarget(name: "lab", dependencies: ["ElkSwift"]),
    ]
)
