// Native NSMenu observer for the macOS tray (17c15). It talks to the real status item of ONE process through the
// Accessibility API: it opens the tracked NSMenu, reads it while it is open (titles, enabled, check marks, submenus),
// presses items and cancels the menu. Most commands are AX-only; the exceptions that DO act on the real session are `clickat`/`rightclick`
// (synthesized mouse events: the pointer moves and is put back; `clickat` re-verifies the element under the point itself and refuses on a
// mismatch), `hover` (pointer move) and `escape` (one global key press, refused unless this pid's pop-up menu window is on screen).
// Build: swiftc -O apps/gui-go/e2e/tray_ax.swift -o <dir>/tray_ax (tray_tracking_run.py builds and hashes it itself).
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
        if let pv = attr(it, kAXPositionAttribute), let sv = attr(it, kAXSizeAttribute) {
            var pp = CGPoint.zero, ss = CGSize.zero
            AXValueGetValue(pv as! AXValue, .cgPoint, &pp); AXValueGetValue(sv as! AXValue, .cgSize, &ss)
            d["frame"] = ["x": Double(pp.x), "y": Double(pp.y), "w": Double(ss.width), "h": Double(ss.height)]
        }
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
/// On-screen windows of the pid at the pop-up menu level or above (>= 101): a menu in AppKit tracking is drawn in such a window. The AX tree can keep a
/// stale AXMenu after tracking ended (17c15 min19), so openness is decided with this count, not with the AX read alone.
func popupWindows(_ pid: pid_t) -> Int {
    let all = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]) ?? []
    return all.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid && (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 101 }.count
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
    print(json(["ok": true, "ownerPid": owner, "mine": owner == pid, "role": str(e, kAXRoleAttribute), "subrole": str(e, kAXSubroleAttribute),
                "title": str(e, kAXTitleAttribute), "description": str(e, kAXDescriptionAttribute), "identifier": str(e, "AXIdentifier"), "help": str(e, kAXHelpAttribute),
                "frame": { () -> [String: Double] in
                    guard let pv = attr(e, kAXPositionAttribute), let sv = attr(e, kAXSizeAttribute) else { return [:] }
                    var pp = CGPoint.zero, ss = CGSize.zero
                    AXValueGetValue(pv as! AXValue, .cgPoint, &pp); AXValueGetValue(sv as! AXValue, .cgSize, &ss)
                    return ["x": Double(pp.x), "y": Double(pp.y), "w": Double(ss.width), "h": Double(ss.height)] }(),
                "attributes": { var a: CFArray?; _ = AXUIElementCopyAttributeNames(e, &a); return (a as? [String]) ?? [] }(), "ns": now()]))
case "agent":
    // Read-only walk of a menu-bar agent process (pid argument = the agent's pid): counts, and only the entries that name this test app or the
    // hidden-items button (other applications' entries are counted, not listed).
    var total = 0
    var hits: [[String: Any]] = []
    func walk2(_ e: AXUIElement, _ depth: Int) {
        total += 1
        let blob = (str(e, kAXTitleAttribute) + "|" + str(e, kAXDescriptionAttribute) + "|" + str(e, "AXIdentifier") + "|" + str(e, kAXHelpAttribute))
        if blob.lowercased().contains("uniclip") || blob.contains("隐藏") || blob.lowercased().contains("hidden") {
            var f: [String: Double]?
            if let pv = attr(e, kAXPositionAttribute), let sv = attr(e, kAXSizeAttribute) {
                var pp = CGPoint.zero, ss = CGSize.zero
                AXValueGetValue(pv as! AXValue, .cgPoint, &pp); AXValueGetValue(sv as! AXValue, .cgSize, &ss)
                f = ["x": Double(pp.x), "y": Double(pp.y), "w": Double(ss.width), "h": Double(ss.height)]
            }
            hits.append(["role": str(e, kAXRoleAttribute), "description": str(e, kAXDescriptionAttribute), "title": str(e, kAXTitleAttribute), "identifier": str(e, "AXIdentifier"), "frame": f ?? [:]])
        }
        if depth > 0 { for c in kids(e) { walk2(c, depth - 1) } }
    }
    walk2(app(pid), 8)
    print(json(["ok": true, "total": total, "hits": hits, "ns": now()]))
case "overflow":
    // The system menu bar's "show hidden menu bar items" button (owned by the menu-bar agent pid): `overflow <agentPid> actions` lists its AX
    // actions, `overflow <agentPid> press` performs AXPress on it (the same as a user clicking it; a second press collapses it).
    func findBtn(_ e: AXUIElement, _ depth: Int) -> AXUIElement? {
        if str(e, kAXRoleAttribute) == "AXButton" && str(e, kAXDescriptionAttribute).contains("隐藏菜单栏项目") { return e }
        if depth > 0 { for c in kids(e) { if let f = findBtn(c, depth - 1) { return f } } }
        return nil
    }
    guard argv.count >= 4, let btn = findBtn(app(pid), 8) else { fail("overflow button not found") }
    var names: CFArray?
    _ = AXUIElementCopyActionNames(btn, &names)
    if argv[3] == "actions" { print(json(["ok": true, "actions": (names as? [String]) ?? [], "description": str(btn, kAXDescriptionAttribute), "ns": now()])) }
    else {
        let r = AXUIElementPerformAction(btn, kAXPressAction as CFString)
        print(json(["ok": r == .success, "axError": r.rawValue, "ns": now()]))
    }
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
case "escape":
    // escape <pid>: one Escape key press (the native way to dismiss a tracked menu). It is a global HID event, so it is sent only if THIS pid's
    // pop-up menu window is on screen at this very moment (a menu in tracking owns the keyboard); otherwise nothing is sent.
    guard argv.count >= 3 else { fail("escape <pid>") }
    if popupWindows(pid) < 1 { print(json(["ok": false, "refused": "no pop-up menu window of this pid is on screen; no key sent", "ns": now()])); exit(0) }
    for down in [true, false] { CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: down)?.post(tap: .cghidEventTap); Thread.sleep(forTimeInterval: 0.05) }
    print(json(["ok": true, "key": "escape", "ns": now()]))
case "actions":
    // actions <pid> <title> [<title>..]: the AX action names of the menu item at the title path.
    guard let m = openMenu(pid), let it = find(m, Array(argv.dropFirst(3))) else { fail("no item at path \(argv.dropFirst(3))") }
    var names: CFArray?
    _ = AXUIElementCopyActionNames(it, &names)
    print(json(["ok": true, "actions": (names as? [String]) ?? [], "role": str(it, kAXRoleAttribute), "ns": now()]))
case "perform":
    // perform <pid> <action> <title> [<title>..]: one named AX action on the menu item at the title path.
    guard argv.count >= 5, let m = openMenu(pid), let it = find(m, Array(argv.dropFirst(4))) else { fail("no item at that path") }
    let r = AXUIElementPerformAction(it, argv[3] as CFString)
    print(json(["ok": r == .success, "axError": r.rawValue, "action": argv[3], "ns": now()]))
case "hover":
    // hover <pid> <x> <y>: move the pointer onto a point the CALLER verified (no click) and put it back after 1.5 s.
    guard argv.count >= 5, let px = Double(argv[3]), let py = Double(argv[4]) else { fail("hover <pid> <x> <y>") }
    let saved = CGEvent(source: nil)?.location ?? CGPoint(x: px, y: py)
    for (dx, dy) in [(-6.0, 0.0), (-2.0, 0.0), (0.0, 0.0)] {
        CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: CGPoint(x: px + dx, y: py + dy), mouseButton: .left)?.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: 0.12)
    }
    Thread.sleep(forTimeInterval: 1.0)
    print(json(["ok": true, "target": ["x": px, "y": py], "ns": now(), "popupWindows": popupWindows(pid)]))
    CGWarpMouseCursorPosition(saved)
case "clickat":
    // clickat <pid> <x> <y> left|right <expect>: an ordinary synthesized click. The element under the point is re-verified HERE, in the same
    // call, right before the events: it must be owned by <pid> or its title/description/identifier must contain <expect>; otherwise nothing is
    // clicked. Moves onto the point, holds the button, releases, and puts the pointer back.
    guard argv.count >= 7, let px = Double(argv[3]), let py = Double(argv[4]) else { fail("clickat <pid> <x> <y> left|right <expect>") }
    var hitEl: AXUIElement?
    let hr = AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), Float(px), Float(py), &hitEl)
    var hitOwner: pid_t = 0
    if let h = hitEl { AXUIElementGetPid(h, &hitOwner) }
    let hitBlob = hitEl.map { str($0, kAXTitleAttribute) + "|" + str($0, kAXDescriptionAttribute) + "|" + str($0, "AXIdentifier") } ?? ""
    if hr != .success || !(hitOwner == pid || hitBlob.contains(argv[6])) {
        print(json(["ok": false, "refused": "element under the point is not the expected target; nothing clicked", "ownerPid": hitOwner, "blob": hitBlob, "ns": now()])); exit(0)
    }
    let right = argv[5] == "right"
    let target = CGPoint(x: px, y: py)
    let saved = CGEvent(source: nil)?.location ?? target
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: target, mouseButton: .left)?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.25)
    CGEvent(mouseEventSource: nil, mouseType: right ? .rightMouseDown : .leftMouseDown, mouseCursorPosition: target, mouseButton: right ? .right : .left)?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.15)
    CGEvent(mouseEventSource: nil, mouseType: right ? .rightMouseUp : .leftMouseUp, mouseCursorPosition: target, mouseButton: right ? .right : .left)?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.3)
    CGWarpMouseCursorPosition(saved)
    print(json(["ok": true, "target": ["x": px, "y": py], "button": argv[5], "ns": now()]))
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
    print(json(["ok": true, "ns": now(), "popupWindows": popupWindows(pid), "menu": tree(m)]))
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
        if let m = openMenu(pid) { print(json(["ok": true, "ns": now(), "popupWindows": popupWindows(pid), "menu": tree(m)])) } else { print(json(["ok": false, "ns": now(), "popupWindows": popupWindows(pid), "error": "no open menu"])) }
        fflush(stdout)
        Thread.sleep(forTimeInterval: ms / 1000)
    }
default:
    fail("unknown command \(argv[1])")
}
