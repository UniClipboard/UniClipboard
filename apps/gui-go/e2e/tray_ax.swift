// Native NSMenu observer for the macOS tray (17c15). It talks to the real status item of ONE process through the
// Accessibility API: it opens the tracked NSMenu, reads it while it is open (titles, enabled, check marks, submenus),
// presses items and cancels the menu. It never moves the pointer and sends no key events.
//
//   swift tray_ax.swift display 0                        main display asleep/active/online (a sleeping display makes screenshots black and may stop menu tracking)
//   swift tray_ax.swift items  <pid>                      status items (AXExtrasMenuBar) of the process
//   swift tray_ax.swift describe <pid>                   roles, actions, attributes and frames of the status items (diagnostic)
//   swift tray_ax.swift open   <pid>                      AXShowMenu on its first status item
//   swift tray_ax.swift read   <pid>                      the open menu tree as JSON (fails if no menu is open)
//   swift tray_ax.swift press  <pid> <title> [<title>..]  AXPress the item at the title path (submenu entries first)
//   swift tray_ax.swift cancel <pid>                      AXCancel on the open menu
//   swift tray_ax.swift watch  <pid> <seconds> <ms>       read the open menu every <ms> ms for <seconds>, one JSON line per read (with a monotonic wall clock in ns)
//
// Every attribute call has a 5 s AX timeout, so a hung target shows up as an error line instead of hanging the observer.
import ApplicationServices
import Foundation

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: CFTypeRef?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? (v as AnyObject?) : nil
}
func err(_ e: AXUIElement, _ name: String) -> AXError {
    var v: CFTypeRef?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v)
}
func kids(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }
func str(_ e: AXUIElement, _ name: String) -> String { (attr(e, name) as? String) ?? "" }

func app(_ pid: pid_t) -> AXUIElement {
    let a = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(a, 5)
    return a
}
func statusItems(_ pid: pid_t) -> [AXUIElement] {
    guard let bar = attr(app(pid), "AXExtrasMenuBar") else { return [] }
    return kids(bar as! AXUIElement)
}
/// The open NSMenu hangs below the status item (AXMenu child) while it is tracked.
func openMenu(_ pid: pid_t) -> AXUIElement? {
    for item in statusItems(pid) {
        if let m = kids(item).first(where: { str($0, kAXRoleAttribute) == "AXMenu" }) { return m }
    }
    return nil
}
func tree(_ menu: AXUIElement) -> [[String: Any]] {
    kids(menu).map { it in
        var d: [String: Any] = ["role": str(it, kAXRoleAttribute), "title": str(it, kAXTitleAttribute),
                                "enabled": (attr(it, kAXEnabledAttribute) as? Bool) ?? true]
        let mark = str(it, "AXMenuItemMarkChar")
        if !mark.isEmpty { d["mark"] = mark }
        if let sub = kids(it).first(where: { str($0, kAXRoleAttribute) == "AXMenu" }) { d["items"] = tree(sub) }
        return d
    }
}
func find(_ menu: AXUIElement, _ path: [String]) -> AXUIElement? {
    guard let head = path.first else { return nil }
    guard let item = kids(menu).first(where: { str($0, kAXTitleAttribute) == head }) else { return nil }
    if path.count == 1 { return item }
    guard let sub = kids(item).first(where: { str($0, kAXRoleAttribute) == "AXMenu" }) else { return nil }
    return find(sub, Array(path.dropFirst()))
}
func now() -> UInt64 { UInt64(Date().timeIntervalSince1970 * 1e9) }
func json(_ o: Any) -> String {
    String(data: try! JSONSerialization.data(withJSONObject: o, options: [.sortedKeys]), encoding: .utf8)!
}
func fail(_ msg: String) -> Never {
    print(json(["ok": false, "error": msg, "ns": now()]))
    exit(1)
}

let argv = CommandLine.arguments
guard argv.count >= 3, let pidNum = Int32(argv[2]) else { fail("usage: tray_ax.swift <cmd> <pid> ...") }
guard AXIsProcessTrusted() else { fail("this process is not trusted for Accessibility") }
let pid = pidNum
switch argv[1] {
case "items":
    let items = statusItems(pid)
    print(json(["ok": !items.isEmpty, "count": items.count, "ns": now(),
                "items": items.map { ["role": str($0, kAXRoleAttribute), "title": str($0, kAXTitleAttribute), "help": str($0, kAXHelpAttribute)] }]))
case "elementat":
    // The accessibility element the system reports under a screen point, and the pid that owns it: the hit test that works for status items
    // (on this macOS they are not windows of the app, so the window server's frontmost window at the point is the menu bar, not the item).
    guard argv.count >= 5, let px = Float(argv[3]), let py = Float(argv[4]) else { fail("elementat <pid> <x> <y>") }
    var el: AXUIElement?
    let r = AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), px, py, &el)
    guard r == .success, let e = el else { print(json(["ok": false, "axError": r.rawValue, "ns": now()])); exit(0) }
    var owner: pid_t = 0
    AXUIElementGetPid(e, &owner)
    print(json(["ok": true, "ownerPid": owner, "mine": owner == pid, "role": str(e, kAXRoleAttribute), "subrole": str(e, kAXSubroleAttribute), "ns": now()]))
case "windows":
    // The window server's view of this pid's windows (status item window, menu windows): layer, bounds, on screen, alpha.
    let all = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
    let mine = all.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }.map { w -> [String: Any] in
        ["layer": w[kCGWindowLayer as String] ?? -1, "onscreen": w[kCGWindowIsOnscreen as String] ?? false, "alpha": w[kCGWindowAlpha as String] ?? -1,
         "bounds": w[kCGWindowBounds as String] ?? [:], "name": w[kCGWindowName as String] ?? ""]
    }
    print(json(["ok": true, "ns": now(), "windows": mine]))
case "hittest":
    // The frontmost on-screen window at a point (front to back as the window server orders them): its owner pid and layer. Foreign owners are
    // reported by pid and layer only.
    guard argv.count >= 5, let px = Double(argv[3]), let py = Double(argv[4]) else { fail("hittest <pid> <x> <y>") }
    let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]) ?? []
    var top: [String: Any] = ["ownerPid": -1]
    for w in list {
        guard let b = w[kCGWindowBounds as String] as? [String: Any], let bx = b["X"] as? Double, let by = b["Y"] as? Double, let bw = b["Width"] as? Double, let bh = b["Height"] as? Double else { continue }
        if px >= bx && px < bx + bw && py >= by && py < by + bh && ((w[kCGWindowAlpha as String] as? Double) ?? 1) > 0 {
            top = ["ownerPid": (w[kCGWindowOwnerPID as String] as? Int32) ?? -1, "layer": w[kCGWindowLayer as String] ?? -1, "mine": ((w[kCGWindowOwnerPID as String] as? Int32) ?? -1) == pid]
            break
        }
    }
    print(json(["ok": true, "ns": now(), "top": top]))
case "display":
    let main = CGMainDisplayID()
    print(json(["ok": true, "asleep": CGDisplayIsAsleep(main) != 0, "active": CGDisplayIsActive(main) != 0, "online": CGDisplayIsOnline(main) != 0, "ns": now()]))
case "scan":
    // Every AXMenu / AXMenuItem / AXWindow reachable from the application element (a diagnostic for where an open menu hangs in the tree).
    var found: [[String: Any]] = []
    func walk(_ e: AXUIElement, _ path: String, _ depth: Int) {
        let role = str(e, kAXRoleAttribute)
        if ["AXMenu", "AXMenuItem", "AXWindow", "AXMenuBar", "AXMenuBarItem"].contains(role) { found.append(["path": path, "role": role, "title": str(e, kAXTitleAttribute)]) }
        if depth == 0 { return }
        for (i, c) in kids(e).enumerated() { walk(c, path + "/" + String(i), depth - 1) }
    }
    let root = app(pid)
    walk(root, "app", 6)
    for key in ["AXMenuBar", "AXExtrasMenuBar", "AXWindows", "AXFocusedUIElement", "AXFocusedWindow"] {
        if let v = attr(root, key) {
            if let el = v as? [AXUIElement] { for (i, c) in el.enumerated() { walk(c, key + "[" + String(i) + "]", 6) } }
            else if CFGetTypeID(v) == AXUIElementGetTypeID() { walk(v as! AXUIElement, key, 6) }
        }
    }
    print(json(["ok": true, "ns": now(), "found": found]))
case "describe":
    func actions(_ e: AXUIElement) -> [String] {
        var a: CFArray?
        return AXUIElementCopyActionNames(e, &a) == .success ? (a as? [String] ?? []) : []
    }
    func names(_ e: AXUIElement) -> [String] {
        var a: CFArray?
        return AXUIElementCopyAttributeNames(e, &a) == .success ? (a as? [String] ?? []) : []
    }
    func pos(_ e: AXUIElement) -> [String: Double]? {
        var p = CGPoint.zero, s = CGSize.zero
        guard let pv = attr(e, kAXPositionAttribute), let sv = attr(e, kAXSizeAttribute) else { return nil }
        AXValueGetValue(pv as! AXValue, .cgPoint, &p)
        AXValueGetValue(sv as! AXValue, .cgSize, &s)
        return ["x": Double(p.x), "y": Double(p.y), "w": Double(s.width), "h": Double(s.height)]
    }
    func d(_ e: AXUIElement, _ depth: Int) -> [String: Any] {
        var o: [String: Any] = ["role": str(e, kAXRoleAttribute), "subrole": str(e, kAXSubroleAttribute), "title": str(e, kAXTitleAttribute),
                                "actions": actions(e), "attributes": names(e)]
        if let p = pos(e) { o["frame"] = p }
        if depth > 0 { o["children"] = kids(e).map { d($0, depth - 1) } }
        return o
    }
    print(json(["ok": true, "ns": now(), "items": statusItems(pid).map { d($0, 2) }]))
case "rightclick":
    // A real right mouse click at the centre of the status item: Wails' pre-click monitor routes it into native menu tracking (the left
    // button runs the app's own click handler instead). It moves the real pointer for a moment and puts it back.
    guard let item = statusItems(pid).first, let pv = attr(item, kAXPositionAttribute), let sv = attr(item, kAXSizeAttribute) else { fail("no status item frame") }
    var p = CGPoint.zero, sz = CGSize.zero
    AXValueGetValue(pv as! AXValue, .cgPoint, &p)
    AXValueGetValue(sv as! AXValue, .cgSize, &sz)
    let target = CGPoint(x: p.x + sz.width / 2, y: p.y + sz.height / 2)
    let saved = CGEvent(source: nil)?.location ?? target
    let down = CGEvent(mouseEventSource: nil, mouseType: .rightMouseDown, mouseCursorPosition: target, mouseButton: .right)
    let up = CGEvent(mouseEventSource: nil, mouseType: .rightMouseUp, mouseCursorPosition: target, mouseButton: .right)
    // Move onto the item first (a pointer that never entered the status window is not what a user's click looks like), hold the button, release.
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: target, mouseButton: .left)?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.25)
    down?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.15)
    up?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.3)
    CGWarpMouseCursorPosition(saved)
    print(json(["ok": true, "target": ["x": target.x, "y": target.y], "restored": ["x": saved.x, "y": saved.y], "ns": now()]))
case "open":
    guard let item = statusItems(pid).first else { fail("no status item") }
    let r = AXUIElementPerformAction(item, "AXShowMenu" as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "action": "AXShowMenu", "ns": now()]))
case "read":
    guard let m = openMenu(pid) else { fail("no open menu") }
    print(json(["ok": true, "ns": now(), "menu": tree(m)]))
case "press":
    guard let m = openMenu(pid) else { fail("no open menu") }
    guard let it = find(m, Array(argv.dropFirst(3))) else { fail("no item at path \(argv.dropFirst(3))") }
    let r = AXUIElementPerformAction(it, kAXPressAction as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "ns": now()]))
case "cancel":
    guard let m = openMenu(pid) else { fail("no open menu") }
    let r = AXUIElementPerformAction(m, kAXCancelAction as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "ns": now()]))
case "watch":
    guard argv.count >= 5, let seconds = Double(argv[3]), let ms = Double(argv[4]) else { fail("watch <pid> <seconds> <ms>") }
    let end = Date().addingTimeInterval(seconds)
    while Date() < end {
        if let m = openMenu(pid) { print(json(["ok": true, "ns": now(), "menu": tree(m)])) } else { print(json(["ok": false, "ns": now(), "error": "no open menu"])) }
        fflush(stdout)
        Thread.sleep(forTimeInterval: ms / 1000)
    }
default:
    fail("unknown command \(argv[1])")
}
