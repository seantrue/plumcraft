// swift-tools-version: 6.0
// N0(a) canvas spike: the shared canvas (scene, faces, layer tree, benchmark, both hosts).
import PackageDescription

let package = Package(
    name: "CanvasKit",
    platforms: [.iOS(.v18), .macOS(.v15)],
    products: [.library(name: "CanvasKit", targets: ["CanvasKit"])],
    targets: [
        .target(name: "CanvasKit"),
        .testTarget(name: "CanvasKitTests", dependencies: ["CanvasKit"]),
    ]
)
