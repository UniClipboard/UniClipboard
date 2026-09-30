import AppKit
import CryptoKit

// A dedicated paste receiver keeps test keystrokes away from the user's apps.
final class Target: NSObject, NSApplicationDelegate, NSTextViewDelegate {
    let window = NSWindow(contentRect: NSRect(x: 40, y: 40, width: 480, height: 160), styleMask: [.titled], backing: .buffered, defer: false)
    let editor = NSTextView(frame: NSRect(x: 0, y: 0, width: 480, height: 160))
    let previousApp = NSWorkspace.shared.frontmostApplication
    let savedClipboard: [NSPasteboardItem] = (NSPasteboard.general.pasteboardItems ?? []).map { original in
        let copy = NSPasteboardItem()
        for type in original.types { if let data = original.data(forType: type) { copy.setData(data, forType: type) } }
        return copy
    }
    func emit(_ value: [String: Any]) {
        if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
            FileHandle.standardOutput.write(data + Data([10]))
        }
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        window.title = "UniClipboard automated test target"
        let menu = NSMenu()
        let edit = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: "Edit")
        submenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.submenu = submenu
        menu.addItem(edit)
        NSApp.mainMenu = menu
        editor.delegate = self
        window.contentView = editor
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(editor)
        NSApp.activate(ignoringOtherApps: true)
        emit(["event": "ready", "pid": ProcessInfo.processInfo.processIdentifier])
        DispatchQueue.global().async {
            while let line = readLine() {
                DispatchQueue.main.async {
                    if line.hasPrefix("click ") {
                        let parts = line.split(separator: " ")
                        if parts.count == 4, let x = Double(parts[1]), let y = Double(parts[2]), let pid = Int32(parts[3]) {
                            let point = CGPoint(x: x, y: y)
                            let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
                            // The pointer's own sprite is a window at the top layer; it is not what a click lands on.
                            let hit = windows.first { info in
                                guard (info[kCGWindowLayer as String] as? Int ?? 0) < 1_000_000,
                                      let bounds = info[kCGWindowBounds as String] as? [String: Any],
                                      let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary) else { return false }
                                return rect.contains(point)
                            }
                            if (hit?[kCGWindowOwnerPID as String] as? Int32) == pid {
                                CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
                                CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
                                self.emit(["event": "click", "ok": true])
                            } else { self.emit(["event": "click", "ok": false]) }
                        }
                    }
                    if line == "clipboard" {
                        let text = NSPasteboard.general.string(forType: .string) ?? ""
                        let digest = SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
                        self.emit(["event": "clipboard", "sha256": digest])
                    }
                    if line == "reset" { self.editor.string = "" }
                    if line == "quit" { NSApp.terminate(nil) }
                    if line == "front" {
                        NSApp.activate(ignoringOtherApps: true); self.window.makeKeyAndOrderFront(nil)
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { self.emit(["event": "front", "active": NSApp.isActive]) }
                    }
                }
            }
            DispatchQueue.main.async { NSApp.terminate(nil) }
        }
    }
    func textDidChange(_ notification: Notification) {
        let digest = SHA256.hash(data: Data(editor.string.utf8)).map { String(format: "%02x", $0) }.joined()
        emit(["event": "text", "sha256": digest])
    }
    func applicationWillTerminate(_ notification: Notification) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.writeObjects(savedClipboard)
        previousApp?.activate(options: [])
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let target = Target()
app.delegate = target
app.run()
