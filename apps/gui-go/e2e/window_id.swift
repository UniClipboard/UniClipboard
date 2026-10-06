// Prints the CGWindowID of the largest on-screen window owned by the given PID,
// optionally restricted to windows whose title contains the second argument
// ("-" selects untitled windows, such as the frameless quick panel).
import CoreGraphics
import Foundation

guard CommandLine.arguments.count >= 2, let pid = Int32(CommandLine.arguments[1]) else { exit(2) }
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
let title = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : nil
var best: (id: Int, area: Double)?
for window in list where (window[kCGWindowOwnerPID as String] as? Int32) == pid {
    let name = (window[kCGWindowName as String] as? String) ?? ""
    if let title = title, title == "-" ? !name.isEmpty : !name.contains(title) { continue }
    guard let bounds = window[kCGWindowBounds as String] as? [String: Double] else { continue }
    let area = (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
    if area > (best?.area ?? 0), let id = window[kCGWindowNumber as String] as? Int { best = (id, area) }
}
guard let found = best else { exit(1) }
print(found.id)
