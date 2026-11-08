// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Unic",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Unic", targets: ["UnicApp"])],
    targets: [.executableTarget(name: "UnicApp")]
)
