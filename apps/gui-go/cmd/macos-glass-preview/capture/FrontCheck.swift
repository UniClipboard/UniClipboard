// Lists on-screen windows front to back that intersect a rectangle, so a region capture can be
// trusted to show only the expected windows. Usage: front-check <x> <y> <w> <h>
import CoreGraphics
import Foundation
let a = CommandLine.arguments.dropFirst().map { Double($0)! }
let rect = CGRect(x: a[0], y: a[1], width: a[2], height: a[3])
for w in CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]] {
  guard let b = w[kCGWindowBounds as String] as? [String: Double], (w[kCGWindowLayer as String] as? Int) == 0 else { continue }
  let r = CGRect(x: b["X"]!, y: b["Y"]!, width: b["Width"]!, height: b["Height"]!)
  if r.intersects(rect) { print(w[kCGWindowNumber as String]!, w[kCGWindowOwnerName as String] ?? "") }
}
