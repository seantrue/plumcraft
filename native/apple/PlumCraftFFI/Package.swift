// swift-tools-version: 6.0
// PlumCraft's core for Swift shells (stage N0(b) spike). PlumCraftFFI.xcframework is built from
// crates/ffi by native/apple/build-xcframework.sh and isn't committed.
import PackageDescription

let package = Package(
    name: "PlumCraftFFI",
    platforms: [.iOS(.v18), .macOS(.v15)],
    products: [.library(name: "PlumCraft", targets: ["PlumCraft"])],
    targets: [
        .binaryTarget(name: "PlumCraftFFI", path: "PlumCraftFFI.xcframework"),
        .target(
            name: "PlumCraft",
            dependencies: ["PlumCraftFFI"],
            swiftSettings: [.swiftLanguageMode(.v6)],
            linkerSettings: [.linkedFramework("CoreFoundation"), .linkedFramework("Security"), .linkedLibrary("c++")]
        ),
        .testTarget(name: "PlumCraftTests", dependencies: ["PlumCraft"], swiftSettings: [.swiftLanguageMode(.v6)]),
    ]
)
