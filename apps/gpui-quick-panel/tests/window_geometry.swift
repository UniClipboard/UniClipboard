import CoreGraphics
import Foundation

let owner = CommandLine.arguments.dropFirst().first.flatMap(Int.init) ?? -1
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let panels = windows.filter {
    ($0[kCGWindowOwnerPID as String] as? Int) == owner && ($0[kCGWindowLayer as String] as? Int) == 101
}.map { window in
    ["id": window[kCGWindowNumber as String] ?? 0, "bounds": window[kCGWindowBounds as String] ?? [:]]
}
let data = try JSONSerialization.data(withJSONObject: panels, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
