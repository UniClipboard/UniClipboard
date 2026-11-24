import ApplicationServices
import AppKit

func children(_ element: AXUIElement) throws -> [AXUIElement] {
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value)
    guard status == .success, let elements = value as? [AXUIElement] else {
        throw NSError(domain: "Dock accessibility", code: Int(status.rawValue))
    }
    return elements
}

guard AXIsProcessTrusted() else {
    FileHandle.standardError.write(Data("Dock probe requires Accessibility permission\n".utf8))
    exit(1)
}
if CommandLine.arguments.count == 1 { exit(0) }

let bundle = URL(fileURLWithPath: CommandLine.arguments[1]).resolvingSymlinksInPath().path
guard let dock = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock").first else {
    throw NSError(domain: "Dock process unavailable", code: 1)
}
let application = AXUIElementCreateApplication(dock.processIdentifier)
let items = try children(application).flatMap(children)
let matching = items.filter { item in
    var url: CFTypeRef?
    guard AXUIElementCopyAttributeValue(item, kAXURLAttribute as CFString, &url) == .success,
        let itemURL = url as? URL else { return false }
    return itemURL.resolvingSymlinksInPath().path == bundle
}
print(matching.count)
