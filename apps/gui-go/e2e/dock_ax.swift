// Native observer and actor for the macOS Dock lifecycle of ONE process (issue #2040), used by dock_lifecycle_run.py.
// It reads what the system itself reports (NSRunningApplication activation policy, NSWorkspace frontmost and menu bar owner,
// the Dock's own AX list) and acts only on the target through AX presses (its windows, its app menu, its status item, its Dock
// tile). It never moves the pointer and never sends a key event.
// Build: swiftc -O apps/gui-go/e2e/dock_ax.swift -o <dir>/dock_ax (dock_lifecycle_run.py builds and hashes it itself).
//
//   dock_ax state <pid> <bundlePath>        policy, active/frontmost/menu bar owner, the pid's AX windows, its Dock tile
//   dock_ax close-button <pid> <title>      AXPress the close (red) button of the window with that title
//   dock_ax menu-key <pid> <char>           AXPress the app menu item whose key equivalent is Cmd+<char> (what the key triggers)
//   dock_ax status-press <pid>              AXPress the pid's status item (a click on the tray icon, which opens its menu)
//   dock_ax instances <bundleId>            processes LaunchServices lists for the bundle id: pid, bundle, executable, policy
//   dock_ax minimize <pid> <title>          set AXMinimized on the window with that title
//   dock_ax dock-press <bundlePath>         AXPress the Dock tile of the bundle (a Dock click)
//   dock_ax reopen <pid>                    send the reopen Apple event (kAEReopenApplication) to the pid, what Finder and Spotlight
//                                           send to a running app; addressed by pid, so it can never start another process
//
// Every AX call has a 5 s messaging timeout, so a hung target (a blocked main thread) shows up as an error instead of a hang.
import AppKit
import ApplicationServices
import Foundation

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: CFTypeRef?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? (v as AnyObject?) : nil
}
func kids(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }
func str(_ e: AXUIElement, _ name: String) -> String { (attr(e, name) as? String) ?? "" }
func bool(_ e: AXUIElement, _ name: String) -> Bool { (attr(e, name) as? Bool) ?? false }
func frame(_ e: AXUIElement) -> [Double]? {
    guard let pv = attr(e, kAXPositionAttribute), let sv = attr(e, kAXSizeAttribute) else { return nil }
    var p = CGPoint.zero, s = CGSize.zero
    AXValueGetValue(pv as! AXValue, .cgPoint, &p)
    AXValueGetValue(sv as! AXValue, .cgSize, &s)
    return [p.x, p.y, s.width, s.height].map { Double($0) }
}
func app(_ pid: pid_t) -> AXUIElement {
    let a = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(a, 5)
    return a
}
func json(_ o: Any) -> String {
    String(data: try! JSONSerialization.data(withJSONObject: o, options: [.sortedKeys]), encoding: .utf8)!
}
func fail(_ m: String) -> Never { print(json(["ok": false, "error": m])); exit(1) }
func now() -> UInt64 { DispatchTime.now().uptimeNanoseconds }

func windows(_ pid: pid_t) -> (rows: [[String: Any]], error: Int32) {
    var v: CFTypeRef?
    let r = AXUIElementCopyAttributeValue(app(pid), kAXWindowsAttribute as CFString, &v)
    guard r == .success, let list = v as? [AXUIElement] else { return ([], r.rawValue) }
    return (list.map { w in
        ["title": str(w, kAXTitleAttribute), "main": bool(w, kAXMainAttribute), "focused": bool(w, kAXFocusedAttribute),
         "minimized": bool(w, kAXMinimizedAttribute), "frame": frame(w) ?? []]
    }, 0)
}
func window(_ pid: pid_t, _ title: String) -> AXUIElement? {
    ((attr(app(pid), kAXWindowsAttribute) as? [AXUIElement]) ?? []).first { str($0, kAXTitleAttribute) == title }
}
/// The Dock tile whose AXURL is the bundle (file URL, trailing slash tolerated).
func dockTile(_ bundlePath: String) -> AXUIElement? {
    guard let dock = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock").first else { return nil }
    let want = URL(fileURLWithPath: bundlePath).standardizedFileURL.path
    for list in kids(app(dock.processIdentifier)) where str(list, kAXRoleAttribute) == "AXList" {
        for item in kids(list) {
            if let url = attr(item, kAXURLAttribute) as? URL, url.standardizedFileURL.path == want { return item }
            if let s = attr(item, kAXURLAttribute) as? String, URL(string: s)?.standardizedFileURL.path == want { return item }
        }
    }
    return nil
}

let argv = CommandLine.arguments
guard argv.count >= 3 else { fail("usage: see the header") }
switch argv[1] {
case "state":
    guard argv.count >= 4, let pid = pid_t(argv[2]) else { fail("state <pid> <bundlePath>") }
    let running = NSRunningApplication(processIdentifier: pid)
    let ws = NSWorkspace.shared
    let (rows, axError) = windows(pid)
    var tile: [String: Any] = ["present": false]
    if let t = dockTile(argv[3]) {
        tile = ["present": true, "title": str(t, kAXTitleAttribute), "running": bool(t, "AXIsApplicationRunning"), "frame": frame(t) ?? []]
    }
    print(json(["ok": running != nil, "ns": now(), "running": running != nil, "terminated": running?.isTerminated ?? true,
                "policy": running.map { $0.activationPolicy.rawValue } ?? -1, "active": running?.isActive ?? false,
                "frontmostPid": ws.frontmostApplication?.processIdentifier ?? -1,
                "menuBarOwnerPid": ws.menuBarOwningApplication?.processIdentifier ?? -1,
                "focusedWindow": (attr(app(pid), kAXFocusedWindowAttribute)).map { str($0 as! AXUIElement, kAXTitleAttribute) } ?? NSNull(),
                "windows": rows, "windowsAxError": axError, "dockTile": tile]))
case "close-button":
    guard argv.count >= 4, let pid = pid_t(argv[2]) else { fail("close-button <pid> <title>") }
    guard let w = window(pid, argv[3]) else { fail("no window titled \(argv[3])") }
    guard let b = attr(w, kAXCloseButtonAttribute) else { fail("the window has no close button") }
    let r = AXUIElementPerformAction(b as! AXUIElement, kAXPressAction as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "ns": now()]))
case "menu-key":
    guard argv.count >= 4, let pid = pid_t(argv[2]) else { fail("menu-key <pid> <char>") }
    guard let bar = attr(app(pid), kAXMenuBarAttribute) else { fail("no app menu bar") }
    func search(_ e: AXUIElement) -> AXUIElement? {
        for k in kids(e) {
            if str(k, kAXRoleAttribute) == "AXMenuItem", str(k, "AXMenuItemCmdChar").uppercased() == argv[3].uppercased(),
               (attr(k, "AXMenuItemCmdModifiers") as? Int) == 0 { return k }   // 0: Command only
            if let f = search(k) { return f }
        }
        return nil
    }
    guard let item = search(bar as! AXUIElement) else { fail("no menu item with Cmd+\(argv[3])") }
    let r = AXUIElementPerformAction(item, kAXPressAction as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "title": str(item, kAXTitleAttribute), "enabled": bool(item, kAXEnabledAttribute), "ns": now()]))
case "status-press":
    guard let pid = pid_t(argv[2]) else { fail("status-press <pid>") }
    guard let extras = attr(app(pid), "AXExtrasMenuBar"), let item = kids(extras as! AXUIElement).first else { fail("no status item") }
    let actions: [String] = { var a: CFArray?; AXUIElementCopyActionNames(item, &a); return (a as? [String]) ?? [] }()
    let r = AXUIElementPerformAction(item, kAXPressAction as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "actions": actions, "ns": now()]))
case "instances":
    let apps = NSRunningApplication.runningApplications(withBundleIdentifier: argv[2])
    print(json(["ok": true, "instances": apps.map { ["pid": $0.processIdentifier, "bundle": $0.bundleURL?.path ?? "",
                                                     "executable": $0.executableURL?.path ?? "", "policy": $0.activationPolicy.rawValue] }]))
case "minimize":
    guard argv.count >= 4, let pid = pid_t(argv[2]) else { fail("minimize <pid> <title>") }
    guard let w = window(pid, argv[3]) else { fail("no window titled \(argv[3])") }
    let r = AXUIElementSetAttributeValue(w, kAXMinimizedAttribute as CFString, kCFBooleanTrue)
    print(json(["ok": r == .success, "axError": r.rawValue, "ns": now()]))
case "dock-press":
    guard let t = dockTile(argv[2]) else { fail("no Dock tile for \(argv[2])") }
    let r = AXUIElementPerformAction(t, kAXPressAction as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "ns": now()]))
case "reopen":
    guard let pid = pid_t(argv[2]) else { fail("reopen <pid>") }
    let event = NSAppleEventDescriptor(eventClass: AEEventClass(kCoreEventClass), eventID: AEEventID(kAEReopenApplication),
                                       targetDescriptor: NSAppleEventDescriptor(processIdentifier: pid),
                                       returnID: AEReturnID(kAutoGenerateReturnID), transactionID: AETransactionID(kAnyTransactionID))
    do {
        _ = try event.sendEvent(options: [.noReply], timeout: 5)
        print(json(["ok": true, "ns": now()]))
    } catch {
        print(json(["ok": false, "error": "\(error)", "ns": now()]))
    }
default:
    fail("unknown command \(argv[1])")
}
