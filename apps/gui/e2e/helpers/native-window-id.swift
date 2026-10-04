// Prints the CGWindowID of the largest on-screen window owned by a process, so
// `screencapture -l <id>` can capture just that window (never the whole screen).
// Usage: swift native-window-id.swift <pid>
import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2, let pid = Int(CommandLine.arguments[1]) else {
  FileHandle.standardError.write("usage: native-window-id.swift <pid>\n".data(using: .utf8)!)
  exit(2)
}
let windows =
  CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
  as? [[String: Any]] ?? []
let owned = windows.filter {
  ($0[kCGWindowOwnerPID as String] as? Int) == pid && ($0[kCGWindowLayer as String] as? Int) == 0
}
func area(_ window: [String: Any]) -> Double {
  let bounds = window[kCGWindowBounds as String] as? [String: Double] ?? [:]
  return (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
}
guard let main = owned.max(by: { area($0) < area($1) }),
  let id = main[kCGWindowNumber as String] as? Int
else {
  FileHandle.standardError.write("no on-screen window for pid \(pid)\n".data(using: .utf8)!)
  exit(1)
}
print(id)
