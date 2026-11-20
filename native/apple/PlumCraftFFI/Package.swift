// swift-tools-version: 6.2
// PlumCraft's Swift kit (N3 slice 2): everything the Apple shells need below their views.
//
// Targets, bottom up:
// - PlumCraftFFI: the core as a static library in an xcframework, built from crates/ffi by
//   native/apple/build-xcframework.sh (not committed).
// - PlumCraftModels: pure Swift, no FFI. JSON values, a decoder over them, and the Codable types
//   of the contract (docs/shell-interface.md): the scene and its deltas, theme, menus, asks,
//   watched values, palette items, hits, gestures, the feed.
// - PlumCraft: the FFI wrapper. PlumCore (a Sendable class) opens a core, sends requests and
//   resumes them from the feed, fans the feed out, lends blobs, and makes the typed hot calls.
// - PlumCraftKit: the observable document model (main actor) that keeps the scene by nid with a
//   revision per item, the theme, the menus, pending asks and the watched value; and CanvasLink,
//   the geometry and gesture plumbing a canvas uses.
// - PlumCraftCanvas (N3 slice 3): the layer canvas, a CALayer per face drawn on workers, a shape
//   layer per wire, under one transformed layer; with a thin host per platform (an NSView on
//   macOS, a UIView on iOS) that recognises input and sends it as ui.gesture.
// - PlumCraftConformance: the conformance runner (docs/shell-interface.md 16.1) in Swift, with a
//   driver through the kit and one over the core's control port.
// - plumcraft-conformance: the runner as a command, pointed at a running app's control port.
import PackageDescription

let strict: [SwiftSetting] = [.swiftLanguageMode(.v6)]

let package = Package(
    name: "PlumCraftFFI",
    platforms: [.iOS(.v18), .macOS(.v15)],
    products: [
        .library(name: "PlumCraftKit", targets: ["PlumCraftKit", "PlumCraft", "PlumCraftModels"]),
        .library(name: "PlumCraft", targets: ["PlumCraft", "PlumCraftModels"]),
        .library(name: "PlumCraftCanvas", targets: ["PlumCraftCanvas"]),
        .library(name: "PlumCraftConformance", targets: ["PlumCraftConformance"]),
        .executable(name: "plumcraft-conformance", targets: ["plumcraft-conformance"]),
    ],
    targets: [
        .binaryTarget(name: "PlumCraftFFI", path: "PlumCraftFFI.xcframework"),
        .target(name: "PlumCraftModels", swiftSettings: strict),
        .target(
            name: "PlumCraft",
            dependencies: ["PlumCraftFFI", "PlumCraftModels"],
            swiftSettings: strict,
            linkerSettings: [.linkedFramework("CoreFoundation"), .linkedFramework("Security"), .linkedLibrary("c++")]
        ),
        .target(name: "PlumCraftKit", dependencies: ["PlumCraft", "PlumCraftModels"], swiftSettings: strict),
        .target(name: "PlumCraftCanvas", dependencies: ["PlumCraftKit", "PlumCraft", "PlumCraftModels"], swiftSettings: strict),
        .target(name: "PlumCraftConformance", dependencies: ["PlumCraftKit", "PlumCraft", "PlumCraftModels"], swiftSettings: strict),
        .executableTarget(name: "plumcraft-conformance", dependencies: ["PlumCraftConformance", "PlumCraftModels"], swiftSettings: strict),
        .testTarget(name: "PlumCraftModelsTests", dependencies: ["PlumCraftModels"], swiftSettings: strict),
        .testTarget(name: "PlumCraftTests", dependencies: ["PlumCraft", "PlumCraftKit", "PlumCraftModels"], swiftSettings: strict),
        .testTarget(name: "PlumCraftCanvasTests", dependencies: ["PlumCraftCanvas", "PlumCraftKit", "PlumCraft", "PlumCraftModels"], swiftSettings: strict),
        .testTarget(
            name: "PlumCraftConformanceTests",
            dependencies: ["PlumCraftConformance", "PlumCraftKit", "PlumCraft", "PlumCraftModels"],
            swiftSettings: strict
        ),
    ]
)
