// swift-tools-version: 5.9
import PackageDescription

// SwiftPM manifest exists alongside the Xcode project so the model layer can be
// unit-tested via `swift test` without provisioning a test target inside Xcode.
// The library target reuses `UniClipboard/Models/` directly — the same files
// the Xcode app target picks up via PBXFileSystemSynchronizedRootGroup.

let package = Package(
    name: "UniClipboardModels",
    platforms: [
        .iOS(.v15),
        .macOS(.v12),
    ],
    products: [
        .library(name: "UniClipboardModels", targets: ["UniClipboardModels"]),
        .library(name: "UniClipboardNetwork", targets: ["UniClipboardNetwork"]),
        .library(name: "UniClipboardCache", targets: ["UniClipboardCache"]),
    ],
    targets: [
        .target(
            name: "UniClipboardModels",
            path: "Shared/Models"
        ),
        .testTarget(
            name: "UniClipboardModelsTests",
            dependencies: ["UniClipboardModels"],
            path: "Tests/UniClipboardModelsTests"
            // Fixtures are not copied as bundle resources — the test loads them
            // directly from `docs/examples/` via #file, so any edits to the
            // fixtures land in the next test run with no sync step.
        ),
        .target(
            name: "UniClipboardNetwork",
            dependencies: ["UniClipboardModels"],
            path: "Shared/Network"
        ),
        .testTarget(
            name: "UniClipboardNetworkTests",
            dependencies: ["UniClipboardNetwork", "UniClipboardModels"],
            path: "Tests/UniClipboardNetworkTests"
        ),
        .target(
            name: "UniClipboardCache",
            dependencies: ["UniClipboardModels"],
            path: "Shared/Cache"
        ),
        .testTarget(
            name: "UniClipboardCacheTests",
            dependencies: ["UniClipboardCache", "UniClipboardModels"],
            path: "Tests/UniClipboardCacheTests"
        ),
    ]
)
