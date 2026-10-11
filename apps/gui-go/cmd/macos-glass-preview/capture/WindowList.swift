// Prints on-screen window ids, owners and titles (no screen-recording permission needed for these fields).
import CoreGraphics
import Foundation
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list {
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  let name = w[kCGWindowName as String] as? String ?? ""
  if owner.lowercased().contains(CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "glass") {
    print(w[kCGWindowNumber as String]!, owner, "|", name, w[kCGWindowBounds as String]!)
  }
}
