import SwiftUI
import AppKit

struct Clip: Identifiable, Equatable {
    let id: String
    let kind: Kind
    let app: String
    let device: String
    let day: String
    let time: String
    var body = ""
    var name = ""          // file name for images and files
    var art = 0            // sample artwork variant for images
    var tags: [String] = []
    var pinned = false
    var deleted = false

    var title: String {
        switch kind {
        case .image, .file: return name
        case .link: return host + path
        case .code: return body.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        default: return body.split(separator: "\n").first.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        }
    }
    var host: String { URL(string: body)?.host?.replacingOccurrences(of: "www.", with: "") ?? body }
    var path: String { URL(string: body)?.path ?? "" }
    var size: String {
        switch kind {
        case .image: return "2.4 MB"
        case .file: return "84 KB"
        default: return ByteCountFormatter.string(fromByteCount: Int64(body.utf8.count), countStyle: .file)
        }
    }
    var copyText: String { body.isEmpty ? name : body }
    /// Relative age against the mock "now" (Today 22:30).
    var ago: String {
        guard day == "Today" else { return day }
        let p = time.split(separator: ":").compactMap { Int($0) }
        guard p.count == 2 else { return time }
        let minutes = 22 * 60 + 30 - (p[0] * 60 + p[1])
        return minutes < 60 ? "\(max(minutes, 1)) min ago" : "\(minutes / 60) hr ago"
    }

    enum Kind: String, CaseIterable {
        case text, link, code, image, file
        var name: String {
            switch self {
            case .text: return "Text"
            case .link: return "Links"
            case .code: return "Code"
            case .image: return "Images"
            case .file: return "Files"
            }
        }
        var singular: String {
            switch self {
            case .link: return "Link"
            case .image: return "Image"
            case .file: return "File"
            default: return name
            }
        }
        var symbol: String {
            switch self {
            case .text: return "text.alignleft"
            case .link: return "link"
            case .code: return "chevron.left.forwardslash.chevron.right"
            case .image: return "photo"
            case .file: return "doc.fill"
            }
        }
        /// Outline variant for navigation, where symbols stay monochrome.
        var outline: String { self == .file ? "doc" : symbol }
        var tint: Color {
            switch self {
            case .text: return .gray
            case .link: return .blue
            case .code: return .purple
            case .image: return .orange
            case .file: return .red
            }
        }
    }
}

struct Device: Identifiable, Hashable {
    enum State { case this, connected, relay, offline }
    let id: String
    let symbol: String
    var state: State
    var platform = ""
    var lastSeen = ""
    /// Pairing fingerprint, four groups of four hex digits. Not a secret; shown so both sides can compare.
    var fingerprint = ""

    static let seed = [
        Device(id: "MacBook Pro", symbol: "laptopcomputer", state: .this, platform: "macOS 26.2", lastSeen: "Now"),
        Device(id: "iPhone 16", symbol: "iphone", state: .relay, platform: "iOS 26.1", lastSeen: "Now", fingerprint: "A41C 90E2 5D7B 13F8"),
        Device(id: "Windows PC", symbol: "pc", state: .connected, platform: "Windows 11", lastSeen: "Now", fingerprint: "0B6E D2A7 71C4 98FA"),
        Device(id: "Pixel 8", symbol: "smartphone", state: .offline, platform: "Android 16", lastSeen: "Yesterday, 08:40", fingerprint: "E93D 4A10 C7B2 6F05"),
    ]
    static func symbol(for name: String) -> String { seed.first { $0.id == name }?.symbol ?? "desktopcomputer" }

    var isOnline: Bool { state == .connected || state == .relay }
    var status: String {
        switch state {
        case .this: return "This Mac"
        case .connected: return "Connected"
        case .relay: return "Connected via relay"
        case .offline: return "Offline"
        }
    }
    var color: Color {
        switch state {
        case .this, .connected: return .green
        case .relay: return .orange
        case .offline: return .gray
        }
    }
    func delivery(of clip: Clip) -> String {
        if clip.device == id { return "Source" }
        switch state {
        case .this, .connected: return "Delivered"
        case .relay: return "Delivered via relay"
        case .offline: return "Waiting"
        }
    }
}

/// A device that found this Mac with a pairing code. It stays here until the user approves it.
struct PendingDevice: Identifiable, Equatable {
    let id: String
    let symbol: String
    let platform: String
    let fingerprint: String
}

struct Plugin: Identifiable, Equatable {
    enum Runtime: String { case go = "Go", lua = "Lua" }
    enum Stage: String, CaseIterable { case processing = "Processing", sync = "Sync" }
    let id: String            // reserved, dotted identifier, e.g. builtin.tailcat
    let name: String
    let summary: String
    let runtime: Runtime
    let stage: Stage
    let version: String
    var source = "Local"
    var builtin = false
    var enabled = true

    var symbol: String { stage == .sync ? "arrow.triangle.2.circlepath" : "puzzlepiece.extension" }
    var tint: Color { builtin ? .blue : (runtime == .go ? .teal : .indigo) }

    static let samples: [Plugin] = [
        Plugin(id: "example.normalize", name: "Normalize Text", summary: "Trims trailing spaces and unifies line endings in copied text.",
               runtime: .lua, stage: .processing, version: "0.2.0"),
        Plugin(id: "example.strip-tracking", name: "Strip Tracking Parameters", summary: "Removes utm_* and similar parameters from copied links.",
               runtime: .lua, stage: .processing, version: "0.1.3", source: "github.com/acme/unic-strip-tracking"),
        Plugin(id: "example.redact", name: "Mask Secrets", summary: "Detects API keys and tokens and keeps them out of synced history.",
               runtime: .go, stage: .processing, version: "1.0.0", source: "github.com/acme/unic-redact", enabled: false),
        Plugin(id: "builtin.tailcat", name: "Tailcat Sync", summary: "Syncs the clipboard with your other devices over an end-to-end encrypted tailcat network.",
               runtime: .go, stage: .sync, version: "0.1.0", source: "Built in", builtin: true),
    ]
}

/// What the gallery shows. One source of truth for every filter the pills apply.
enum Filter: Hashable {
    case all, pinned, kind(Clip.Kind), device(String), tag(String), deleted

    var title: String {
        switch self {
        case .all: return "History"
        case .pinned: return "Pinned"
        case .kind(let k): return k.name
        case .device(let d): return d
        case .tag(let t): return t
        case .deleted: return "Recently Deleted"
        }
    }
    var symbol: String {
        switch self {
        case .all: return "clock"
        case .pinned: return "pin"
        case .kind(let k): return k.outline
        case .device(let d): return Device.symbol(for: d)
        case .tag: return "tag"
        case .deleted: return "trash"
        }
    }
    func matches(_ clip: Clip) -> Bool {
        guard clip.deleted == (self == .deleted) else { return false }
        switch self {
        case .all, .deleted: return true
        case .pinned: return clip.pinned
        case .kind(let k): return clip.kind == k
        case .device(let d): return clip.device == d
        case .tag(let t): return clip.tags.contains(t)
        }
    }
}

@MainActor final class Library: ObservableObject {
    static let shared = Library()

    @Published var clips: [Clip] = Library.samples
    @Published var filter: Filter = .all
    @Published var settingsPane: SettingsPane = .general
    @Published var devices = Device.seed
    @Published var pending = [PendingDevice(id: "Work Laptop", symbol: "laptopcomputer", platform: "Windows 11", fingerprint: "7F3A 09C2 B81D 44E6")]
    @Published var plugins = Plugin.samples
    @Published var restartRequired = false
    @Published var showInspector = false
    /// User-defined filter pills, in display order. A tag may exist with no items yet.
    @Published var customTags = ["deploy", "design", "work", "finance"]
    @Published var selection: String? = "compose-up"
    @Published var previewing: String?
    @Published var query = ""
    @Published var notice: String?
    @Published var paused = false
    @Published var panelSession = UUID()
    private var noticeTask: Task<Void, Never>?

    var live: [Clip] { clips.filter { !$0.deleted } }
    var targets: [Device] { devices.filter(\.isOnline) }
    var onlineCount: Int { targets.count }
    var showDeleted: Bool { filter == .deleted }

    var items: [Clip] { clips.filter { filter.matches($0) && Self.matches($0, query) } }
    func count(_ f: Filter) -> Int { clips.filter(f.matches).count }
    func search(_ q: String) -> [Clip] {
        let found = live.filter { Self.matches($0, q) }
        return found.filter(\.pinned) + found.filter { !$0.pinned }
    }
    private static func matches(_ c: Clip, _ q: String) -> Bool {
        q.isEmpty || "\(c.title) \(c.body) \(c.app) \(c.device) \(c.tags.joined(separator: " "))".localizedCaseInsensitiveContains(q)
    }

    /// Pinned items get their own section at the top of "All"; the rest group by day.
    var sections: [(title: String, clips: [Clip])] {
        let all = items
        let pinnedFirst = filter == .all
        let pinned = pinnedFirst ? all.filter(\.pinned) : []
        let rest = pinnedFirst ? all.filter { !$0.pinned } : all
        var order: [String] = []
        for c in rest where !order.contains(c.day) { order.append(c.day) }
        let days = order.map { d in (title: d, clips: rest.filter { $0.day == d }) }
        return pinned.isEmpty ? days : [(title: "Pinned", clips: pinned)] + days
    }
    var orderedIDs: [String] { sections.flatMap { $0.clips.map(\.id) } }
    var current: Clip? { items.first { $0.id == selection } }
    func clip(_ id: String?) -> Clip? { clips.first { $0.id == id } }

    func mutate(_ id: String?, _ action: (inout Clip) -> Void) {
        guard let i = clips.firstIndex(where: { $0.id == id }) else { return }
        action(&clips[i])
    }
    func copy(_ id: String? = nil) {
        guard let clip = clip(id ?? selection) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(clip.copyText, forType: .string)
        notify("Copied to Clipboard")
    }
    static func clean(_ tag: String) -> String {
        tag.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: "#", with: "")
    }
    /// Creates a filter pill and returns its name (an existing one if the name is already taken).
    @discardableResult func addTag(_ raw: String) -> String? {
        let name = Self.clean(raw)
        guard !name.isEmpty else { return nil }
        if let existing = customTags.first(where: { $0.caseInsensitiveCompare(name) == .orderedSame }) { return existing }
        customTags.append(name)
        return name
    }
    func removeTag(_ name: String) {
        customTags.removeAll { $0 == name }
        for i in clips.indices { clips[i].tags.removeAll { $0 == name } }
        if filter == .tag(name) { filter = .all }
    }
    func toggleTag(_ name: String, on id: String?) {
        mutate(id ?? selection) { c in
            if let i = c.tags.firstIndex(of: name) { c.tags.remove(at: i) } else { c.tags.append(name) }
        }
    }
    func togglePin(_ id: String? = nil) { mutate(id ?? selection) { $0.pinned.toggle() } }
    func delete(_ id: String? = nil) {
        let id = id ?? selection
        let ids = orderedIDs
        if let i = ids.firstIndex(where: { $0 == id }) {
            selection = ids.indices.contains(i + 1) ? ids[i + 1] : (i > 0 ? ids[i - 1] : nil)
        }
        mutate(id) { $0.deleted = true }
        notify("Moved to Recently Deleted")
    }
    func restore(_ id: String? = nil) { mutate(id ?? selection) { $0.deleted = false }; notify("Restored") }
    func approve(_ p: PendingDevice) {
        pending.removeAll { $0 == p }
        devices.append(Device(id: p.id, symbol: p.symbol, state: .connected, platform: p.platform, lastSeen: "Now", fingerprint: p.fingerprint))
        notify("Approved \(p.id)")
    }
    func reject(_ p: PendingDevice) { pending.removeAll { $0 == p }; notify("Rejected \(p.id)") }
    func remove(_ d: Device) { devices.removeAll { $0.id == d.id }; notify("Removed \(d.id)") }

    func togglePlugin(_ id: String) {
        guard let i = plugins.firstIndex(where: { $0.id == id }) else { return }
        plugins[i].enabled.toggle()
        restartRequired = true
    }
    func uninstall(_ id: String) { plugins.removeAll { $0.id == id }; restartRequired = true }
    /// Mock install from a GitHub repo ("owner/repo"); the plugin arrives disabled.
    func install(repo: String) {
        let name = repo.split(separator: "/").last.map(String.init) ?? repo
        plugins.insert(Plugin(id: "github." + name, name: name, summary: "Installed from GitHub.", runtime: .lua, stage: .processing,
                              version: "0.1.0", source: "github.com/" + repo, enabled: false), at: 0)
        notify("Installed \(name)")
    }
    func send(_ id: String?, to device: String) { notify("Sent to \(device)") }
    func notify(_ message: String) {
        noticeTask?.cancel()
        notice = message
        noticeTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(1.8))
            guard !Task.isCancelled else { return }
            self?.notice = nil
        }
    }

    static let samples: [Clip] = [
        Clip(id: "compose-up", kind: .code, app: "Terminal", device: "Windows PC", day: "Today", time: "22:14",
             body: "docker compose -f compose.prod.yml \\\n  up -d --build --remove-orphans", tags: ["deploy"], pinned: true),
        Clip(id: "email", kind: .text, app: "Mail", device: "iPhone 16", day: "Today", time: "21:40", body: "hello@unic.dev", pinned: true),
        Clip(id: "rebase", kind: .code, app: "Xcode", device: "MacBook Pro", day: "Today", time: "21:02", body: "git rebase -i HEAD~3"),
        Clip(id: "repo", kind: .link, app: "Safari", device: "iPhone 16", day: "Today", time: "20:47", body: "https://github.com/UniClipboard/UniClipboard"),
        Clip(id: "screenshot", kind: .image, app: "Screenshot", device: "MacBook Pro", day: "Today", time: "20:31", name: "Screenshot 2026-11-07 at 20.31.02.png", tags: ["design"]),
        Clip(id: "notes", kind: .text, app: "Notes", device: "Windows PC", day: "Today", time: "18:05",
             body: "Design review — Thursday\n• Ship LAN-only mode first\n• Per-device rules after that\n• Revisit the pairing copy", tags: ["work"]),
        Clip(id: "json", kind: .code, app: "VS Code", device: "MacBook Pro", day: "Today", time: "16:12",
             body: "{\n  \"device\": \"pixel-8\",\n  \"relay\": true,\n  \"port\": 41641\n}"),
        Clip(id: "hig", kind: .link, app: "Safari", device: "MacBook Pro", day: "Yesterday", time: "19:20", body: "https://developer.apple.com/design/human-interface-guidelines"),
        Clip(id: "sunrise", kind: .image, app: "Photos", device: "iPhone 16", day: "Yesterday", time: "07:55", name: "IMG_2041.HEIC", art: 1),
        Clip(id: "invoice", kind: .file, app: "Files", device: "Pixel 8", day: "Yesterday", time: "08:40",
             body: "INVOICE  #0921\n\nDesign services — September\n\nHours        32\nRate         $75\n\nTotal        $2,400.00", name: "invoice-0921.pdf", tags: ["finance"]),
    ]
}
