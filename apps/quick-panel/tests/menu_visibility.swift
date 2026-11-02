import CoreGraphics
import Foundation

// Sample the actual native window opacity while keyboard navigation runs.
let windowID = CommandLine.arguments[1]
let duration = Double(CommandLine.arguments[2]) ?? 10
let start = Date()
var samples: [[String: Any]] = []
while Date().timeIntervalSince(start) < duration {
    let windows = CGWindowListCopyWindowInfo(.optionAll, kCGNullWindowID) as? [[String: Any]] ?? []
    let window = windows.first { String(describing: $0[kCGWindowNumber as String] ?? "") == windowID }
    samples.append([
        "elapsed": Date().timeIntervalSince(start),
        "alpha": window?[kCGWindowAlpha as String] ?? -1,
        "onscreen": window?[kCGWindowIsOnscreen as String] ?? false,
    ])
    Thread.sleep(forTimeInterval: 0.005)
}
let data = try JSONSerialization.data(withJSONObject: samples, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
