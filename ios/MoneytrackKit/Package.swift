// swift-tools-version: 6.0
//
// The Rust below the iOS app, as a package the app target and the tests both depend on.
//
// The XCFramework is a build artifact, not a checked-in binary: `tools/ffi-build.sh` writes
// it, and it carries three slices — device, simulator, and macOS. The macOS one is what
// makes this package testable with `swift test` in a second, instead of only inside a
// simulator.

import PackageDescription

let package = Package(
    name: "MoneytrackKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "MoneytrackKit", targets: ["MoneytrackKit"])
    ],
    targets: [
        .binaryTarget(
            name: "MoneytrackFFI",
            path: "Frameworks/MoneytrackFFI.xcframework"
        ),
        .target(
            name: "MoneytrackKit",
            dependencies: ["MoneytrackFFI"],
            path: "Sources/MoneytrackKit",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "MoneytrackKitTests",
            dependencies: ["MoneytrackKit"],
            path: "Tests/MoneytrackKitTests",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
