// Prints the CGWindowID of the largest on-screen window owned by the given PID.
import CoreGraphics
import Foundation

guard CommandLine.arguments.count == 2, let pid = Int32(CommandLine.arguments[1]) else { exit(2) }
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
var best: (id: Int, area: Double)?
for window in list where (window[kCGWindowOwnerPID as String] as? Int32) == pid {
    guard let bounds = window[kCGWindowBounds as String] as? [String: Double] else { continue }
    let area = (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
    if area > (best?.area ?? 0), let id = window[kCGWindowNumber as String] as? Int { best = (id, area) }
}
guard let found = best else { exit(1) }
print(found.id)
