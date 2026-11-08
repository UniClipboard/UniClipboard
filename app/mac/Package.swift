// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Unic",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Unic", targets: ["UnicApp"])],
    targets: [
        .target(name: "SettingsSchema", resources: [.process("Resources")]),
        .executableTarget(name: "UnicApp", dependencies: ["SettingsSchema"]),
        .testTarget(name: "SettingsSchemaTests", dependencies: ["SettingsSchema"])
    ]
)
