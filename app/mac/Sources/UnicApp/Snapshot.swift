#if DEBUG
import AppKit
import SwiftUI

/// Dev aid: render a surface to PNG and quit, without screen-recording permission.
///   UNIC_SNAPSHOT=/tmp/out.png  [UNIC_TARGET=main|panel|preview|menu|settings|plugin-settings]  [UNIC_PANE=devices|plugins]  [UNIC_FILTER=...]  [UNIC_DARK=1]  [UNIC_SELECT=<clip id>]
@MainActor enum Snapshot {
    private static var started = false

    static func scheduleIfRequested() {
        let env = ProcessInfo.processInfo.environment
        guard !started, let path = env["UNIC_SNAPSHOT"] else { return }
        started = true
        let target = env["UNIC_TARGET"] ?? "main"
        if env["UNIC_DARK"] != nil { NSApp.appearance = NSAppearance(named: .darkAqua) }
        switch env["UNIC_PANE"] {
        case "devices": Library.shared.settingsPane = .devices
        case "plugins": Library.shared.settingsPane = .plugins
        default: break
        }
        if let f = env["UNIC_FILTER"] {   // all | pinned | deleted | kind:code | device:iPhone 16 | tag:work
            let parts = f.split(separator: ":", maxSplits: 1).map(String.init)
            switch parts[0] {
            case "pinned": Library.shared.filter = .pinned
            case "deleted": Library.shared.filter = .deleted
            case "kind": Library.shared.filter = .kind(Clip.Kind(rawValue: parts.count > 1 ? parts[1] : "") ?? .text)
            case "device": Library.shared.filter = .device(parts.count > 1 ? parts[1] : "")
            case "tag": Library.shared.filter = .tag(parts.count > 1 ? parts[1] : "")
            default: break
            }
        }
        if let id = env["UNIC_SELECT"] { Library.shared.selection = id; Library.shared.showInspector = true }

        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            var extra: NSWindow?
            switch target {
            case "panel": QuickPanel.shared.show()
            case "preview": Library.shared.previewing = Library.shared.selection
            case "settings": NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            case "plugin-settings":
                let host = NSHostingView(rootView: PluginSettingsPreview(saved: .constant(nil)))
                let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 560, height: 620),
                                 styleMask: [.titled, .closable], backing: .buffered, defer: false)
                w.contentView = host
                w.orderFront(nil)
                extra = w
            case "menu":
                let host = NSHostingView(rootView: MenuPanel().environmentObject(Library.shared))
                let w = NSWindow(contentRect: NSRect(origin: .zero, size: host.fittingSize), styleMask: [.borderless], backing: .buffered, defer: false)
                w.contentView = host
                w.orderFront(nil)
                extra = w
            default: break
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                let main = NSApp.windows.first { $0.styleMask.contains(.titled) && $0.isVisible && !($0 is NSPanel) }
                let win: NSWindow?
                switch target {
                case "panel": win = QuickPanel.shared.window
                case "preview": win = main?.attachedSheet
                case "settings": win = NSApp.windows.first { $0 !== main && $0.styleMask.contains(.titled) && $0.isVisible && !($0 is NSPanel) }
                case "menu", "plugin-settings": win = extra
                default: win = main
                }
                guard let win, let view = win.contentView?.superview ?? win.contentView,
                      let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { exit(2) }
                view.cacheDisplay(in: view.bounds, to: rep)
                try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
                exit(0)
            }
        }
    }
}
#endif
