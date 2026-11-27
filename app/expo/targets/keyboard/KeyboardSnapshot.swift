import CryptoKit
import Foundation
import ImageIO
import UIKit

enum KeyboardItemKind: String, Codable, Hashable {
  case otp
  case url
  case image
  case text
}

enum KeyboardCategoryFilter: Int, CaseIterable, Hashable {
  case all
  case pinned
  case url
  case image

  func matches(_ item: KeyboardClipItem) -> Bool {
    switch self {
    case .all: return true
    case .pinned: return item.pinned
    case .url: return item.kind == .url
    case .image: return item.kind == .image
    }
  }
}

struct KeyboardClipItem: Hashable, Identifiable {
  let id: String
  let revision: String
  let kind: KeyboardItemKind
  let title: String
  let insertText: String?
  let fullText: String?
  let subtitle: String?
  let domain: String?
  let imageMeta: String?
  let thumbnailFile: String?
  let imageFile: String?
  let deviceName: String
  let createdAtMs: Int64
  var pinned: Bool

  var formattedOTPTitle: String {
    guard kind == .otp, title.count == 6 else { return title }
    let idx = title.index(title.startIndex, offsetBy: 3)
    return "\(title[..<idx]) \(title[idx...])"
  }
}

struct KeyboardPreferences: Equatable, Codable {
  var soundEnabled: Bool
  var hapticEnabled: Bool
  var autoSyncEnabled: Bool
  var updatedAtMs: Int64

  enum CodingKeys: String, CodingKey {
    case soundEnabled = "sound_enabled"
    case hapticEnabled = "haptic_enabled"
    case autoSyncEnabled = "auto_sync_enabled"
    case updatedAtMs = "updated_at_ms"
  }

  init(soundEnabled: Bool, hapticEnabled: Bool, autoSyncEnabled: Bool = true, updatedAtMs: Int64) {
    self.soundEnabled = soundEnabled
    self.hapticEnabled = hapticEnabled
    self.autoSyncEnabled = autoSyncEnabled
    self.updatedAtMs = updatedAtMs
  }

  init(from decoder: Decoder) throws {
    let c = try decoder.container(keyedBy: CodingKeys.self)
    self.soundEnabled = try c.decodeIfPresent(Bool.self, forKey: .soundEnabled) ?? true
    self.hapticEnabled = try c.decodeIfPresent(Bool.self, forKey: .hapticEnabled) ?? true
    self.autoSyncEnabled = try c.decodeIfPresent(Bool.self, forKey: .autoSyncEnabled) ?? true
    self.updatedAtMs = try c.decodeIfPresent(Int64.self, forKey: .updatedAtMs) ?? 0
  }

  static let defaults = KeyboardPreferences(soundEnabled: true, hapticEnabled: true, autoSyncEnabled: true, updatedAtMs: 0)
}

struct SharedPasteboardState: Codable {
  let changeCount: Int
  let contentSHA256: String
  let origin: String
  let updatedAtMs: Int64

  enum CodingKeys: String, CodingKey {
    case changeCount = "change_count"
    case contentSHA256 = "content_sha256"
    case origin
    case updatedAtMs = "updated_at_ms"
  }
}

enum KeyboardSnapshotLoadResult {
  case loaded(items: [KeyboardClipItem], generatedAtMs: Int64, revision: String)
  case empty
  case unavailable
}

/// Shared App Group store for the iOS Keyboard Extension.
/// Holds fast first-paint / degraded-mode snapshots, preferences, cross-process
/// anti-loopback pasteboard state, and locates the shared Engine DataDir (0035).
final class KeyboardSnapshotStore {
  private struct RawSnapshot: Codable {
    let version: Int
    let generatedAtMs: Int64
    let historyRevision: String
    let selfDeviceName: String
    let items: [RawItem]

    enum CodingKeys: String, CodingKey {
      case version
      case generatedAtMs = "generated_at_ms"
      case historyRevision = "history_revision"
      case selfDeviceName = "self_device_name"
      case items
    }
  }

  private struct RawItem: Codable {
    let id: String
    let revision: String
    let kind: String
    let title: String
    let insertText: String?
    let fullText: String?
    let subtitle: String?
    let domain: String?
    let imageMeta: String?
    let thumbnailFile: String?
    let imageFile: String?
    let deviceName: String
    let createdAtMs: Int64
    let pinned: Bool

    enum CodingKeys: String, CodingKey {
      case id, revision, kind, title, subtitle, domain, pinned
      case insertText = "insert_text"
      case fullText = "full_text"
      case imageMeta = "image_meta"
      case thumbnailFile = "thumbnail_file"
      case imageFile = "image_file"
      case deviceName = "device_name"
      case createdAtMs = "created_at_ms"
    }
  }

  private struct PinOverrideEntry: Codable {
    let pinned: Bool
    let atMs: Int64

    enum CodingKeys: String, CodingKey {
      case pinned
      case atMs = "at_ms"
    }
  }

  private struct PinOverridesFile: Codable {
    var overrides: [String: PinOverrideEntry]
  }

  private struct PendingAction: Codable {
    let id: String
    let kind: String
    let historyID: String
    let pinned: Bool
    let createdAtMs: Int64

    enum CodingKeys: String, CodingKey {
      case id, kind, pinned
      case historyID = "history_id"
      case createdAtMs = "created_at_ms"
    }
  }

  private struct Heartbeat: Codable {
    let hasFullAccess: Bool
    let lastSeenAtMs: Int64

    enum CodingKeys: String, CodingKey {
      case hasFullAccess = "has_full_access"
      case lastSeenAtMs = "last_seen_at_ms"
    }
  }

  private let fileManager = FileManager.default
  private let thumbnailCache: NSCache<NSString, UIImage> = {
    let cache = NSCache<NSString, UIImage>()
    cache.countLimit = 40
    cache.totalCostLimit = 8 * 1024 * 1024
    return cache
  }()

  var appGroupContainerURL: URL? {
    guard let extID = Bundle.main.bundleIdentifier, extID.hasSuffix(".Keyboard") else {
      return nil
    }
    let mainID = String(extID.dropLast(".Keyboard".count))
    return fileManager.containerURL(forSecurityApplicationGroupIdentifier: "group.\(mainID).incoming-share")
  }

  var sharedEngineDataDir: URL? {
    appGroupContainerURL?
      .appendingPathComponent("Library", isDirectory: true)
      .appendingPathComponent("Application Support", isDirectory: true)
      .appendingPathComponent("unic-v2", isDirectory: true)
  }

  var sharedEngineCacheDir: URL? {
    appGroupContainerURL?
      .appendingPathComponent("Library", isDirectory: true)
      .appendingPathComponent("Caches", isDirectory: true)
      .appendingPathComponent("unic-cache", isDirectory: true)
  }

  private var rootURL: URL? {
    appGroupContainerURL?.appendingPathComponent("keyboard-bridge", isDirectory: true)
  }

  private var snapshotURL: URL? { rootURL?.appendingPathComponent("snapshot.json") }
  private var preferencesURL: URL? { rootURL?.appendingPathComponent("preferences.json") }
  private var heartbeatURL: URL? { rootURL?.appendingPathComponent("heartbeat.json") }
  private var pasteboardStateURL: URL? { rootURL?.appendingPathComponent("pasteboard-state.json") }
  private var pinOverridesURL: URL? { rootURL?.appendingPathComponent("pin-overrides.json") }
  var thumbnailsDir: URL? { rootURL?.appendingPathComponent("thumbnails", isDirectory: true) }
  var imagesDir: URL? { rootURL?.appendingPathComponent("images", isDirectory: true) }
  private var actionsDir: URL? { rootURL?.appendingPathComponent("actions", isDirectory: true) }

  func recordHeartbeat(hasFullAccess: Bool) {
    guard hasFullAccess, let root = rootURL, let url = heartbeatURL else { return }
    try? fileManager.createDirectory(at: root, withIntermediateDirectories: true)
    let hb = Heartbeat(hasFullAccess: hasFullAccess, lastSeenAtMs: Int64(Date().timeIntervalSince1970 * 1000))
    if let data = try? JSONEncoder().encode(hb) {
      try? data.write(to: url, options: [.atomic])
    }
  }

  func loadPreferences() -> KeyboardPreferences {
    guard let url = preferencesURL,
          let data = try? Data(contentsOf: url),
          let prefs = try? JSONDecoder().decode(KeyboardPreferences.self, from: data) else {
      return .defaults
    }
    return prefs
  }

  func loadSnapshot() -> KeyboardSnapshotLoadResult {
    guard let url = snapshotURL else { return .unavailable }
    guard fileManager.fileExists(atPath: url.path) else { return .empty }
    guard let data = try? Data(contentsOf: url),
          let raw = try? JSONDecoder().decode(RawSnapshot.self, from: data) else {
      return .unavailable
    }

    var overrides = loadPinOverrides()
    var pruned = false
    for (id, entry) in overrides where entry.atMs <= raw.generatedAtMs {
      overrides.removeValue(forKey: id)
      pruned = true
    }
    if pruned {
      savePinOverrides(overrides)
    }

    let items: [KeyboardClipItem] = raw.items.map { item in
      let kind = KeyboardItemKind(rawValue: item.kind) ?? .text
      let effectivePinned = overrides[item.id]?.pinned ?? item.pinned
      return KeyboardClipItem(
        id: item.id,
        revision: item.revision,
        kind: kind,
        title: item.title,
        insertText: item.insertText,
        fullText: item.fullText,
        subtitle: item.subtitle,
        domain: item.domain,
        imageMeta: item.imageMeta,
        thumbnailFile: item.thumbnailFile,
        imageFile: item.imageFile,
        deviceName: item.deviceName,
        createdAtMs: item.createdAtMs,
        pinned: effectivePinned
      )
    }
    if items.isEmpty {
      return .empty
    }
    return .loaded(items: items, generatedAtMs: raw.generatedAtMs, revision: raw.historyRevision)
  }

  func enqueuePinChange(historyID: String, pinned: Bool) {
    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)

    var overrides = loadPinOverrides()
    overrides[historyID] = PinOverrideEntry(pinned: pinned, atMs: nowMs)
    savePinOverrides(overrides)

    guard let actions = actionsDir else { return }
    try? fileManager.createDirectory(at: actions, withIntermediateDirectories: true)
    let actionID = UUID().uuidString.lowercased()
    let action = PendingAction(
      id: actionID,
      kind: "set_pin",
      historyID: historyID,
      pinned: pinned,
      createdAtMs: nowMs
    )
    let dest = actions.appendingPathComponent("\(nowMs)-\(actionID).json")
    if let data = try? JSONEncoder().encode(action) {
      try? data.write(to: dest, options: [.atomic])
    }
  }

  func thumbnailImage(named fileName: String) -> UIImage? {
    let key = fileName as NSString
    if let cached = thumbnailCache.object(forKey: key) {
      return cached
    }
    guard let dir = thumbnailsDir else { return nil }
    let fileURL = dir.appendingPathComponent(fileName)
    guard let data = try? Data(contentsOf: fileURL),
          let image = UIImage(data: data) else {
      return nil
    }
    thumbnailCache.setObject(image, forKey: key, cost: data.count)
    return image
  }

  func copyableImage(for item: KeyboardClipItem) -> UIImage? {
    if let imageFile = item.imageFile, let dir = imagesDir {
      let url = dir.appendingPathComponent(imageFile)
      if let data = try? Data(contentsOf: url), let image = UIImage(data: data) {
        return image
      }
    }
    if let thumbFile = item.thumbnailFile {
      return thumbnailImage(named: thumbFile)
    }
    return nil
  }

  func clearImageMemoryCache() {
    thumbnailCache.removeAllObjects()
  }

  // MARK: - Cross-Process Anti-Loopback Pasteboard State

  static func sha256Hex(of data: Data) -> String {
    let digest = SHA256.hash(data: data)
    return digest.map { String(format: "%02x", $0) }.joined()
  }

  static func sha256Hex(ofText text: String) -> String {
    return sha256Hex(of: Data(text.utf8))
  }

  func loadPasteboardState() -> SharedPasteboardState? {
    guard let url = pasteboardStateURL,
          let data = try? Data(contentsOf: url),
          let state = try? JSONDecoder().decode(SharedPasteboardState.self, from: data) else {
      return nil
    }
    return state
  }

  func recordPasteboardState(changeCount: Int, contentSHA256: String, origin: String) {
    guard let root = rootURL, let url = pasteboardStateURL else { return }
    let attrs: [FileAttributeKey: Any] = [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
    try? fileManager.createDirectory(at: root, withIntermediateDirectories: true, attributes: attrs)
    let state = SharedPasteboardState(
      changeCount: changeCount,
      contentSHA256: contentSHA256,
      origin: origin,
      updatedAtMs: Int64(Date().timeIntervalSince1970 * 1000)
    )
    if let data = try? JSONEncoder().encode(state) {
      try? writeAtomic(data: data, to: url)
    }
  }

  func isChangeCountHandled(_ changeCount: Int) -> Bool {
    guard let state = loadPasteboardState() else { return false }
    return state.changeCount == changeCount
  }

  func shouldSuppressCapture(changeCount: Int, contentSHA256: String) -> Bool {
    guard let state = loadPasteboardState() else { return false }
    if state.changeCount == changeCount {
      return true
    }
    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
    if !contentSHA256.isEmpty,
       state.origin == "applied",
       state.contentSHA256 == contentSHA256,
       nowMs - state.updatedAtMs < 60_000 {
      return true
    }
    return false
  }

  // MARK: - Snapshot Writeback (for Fast First Paint & Degraded Mode)

  func saveSnapshot(
    items: [KeyboardClipItem],
    historyRevision: String,
    selfDeviceName: String,
    keepThumbs: Set<String>,
    keepImages: Set<String>
  ) {
    guard let root = rootURL, let url = snapshotURL, let thumbs = thumbnailsDir, let imgs = imagesDir else { return }
    let attrs: [FileAttributeKey: Any] = [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
    for dir in [root, thumbs, imgs] {
      if !fileManager.fileExists(atPath: dir.path) {
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true, attributes: attrs)
      }
    }
    let rawItems = items.map { item in
      RawItem(
        id: item.id,
        revision: item.revision,
        kind: item.kind.rawValue,
        title: item.title,
        insertText: item.insertText,
        fullText: item.fullText,
        subtitle: item.subtitle,
        domain: item.domain,
        imageMeta: item.imageMeta,
        thumbnailFile: item.thumbnailFile,
        imageFile: item.imageFile,
        deviceName: item.deviceName,
        createdAtMs: item.createdAtMs,
        pinned: item.pinned
      )
    }
    let raw = RawSnapshot(
      version: 1,
      generatedAtMs: Int64(Date().timeIntervalSince1970 * 1000),
      historyRevision: historyRevision,
      selfDeviceName: selfDeviceName,
      items: rawItems
    )
    if let data = try? JSONEncoder().encode(raw) {
      try? writeAtomic(data: data, to: url)
    }
    pruneOrphans(in: thumbs, keeping: keepThumbs)
    pruneOrphans(in: imgs, keeping: keepImages)
  }

  func ensureMediaDirs() {
    let attrs: [FileAttributeKey: Any] = [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
    for dir in [rootURL, thumbnailsDir, imagesDir].compactMap({ $0 }) {
      if !fileManager.fileExists(atPath: dir.path) {
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true, attributes: attrs)
      }
    }
  }

  func writeAtomic(data: Data, to url: URL) throws {
    let tmp = url.deletingLastPathComponent().appendingPathComponent(".\(url.lastPathComponent).tmp-\(UUID().uuidString)")
    try data.write(to: tmp, options: [.atomic])
    try? fileManager.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: tmp.path)
    if fileManager.fileExists(atPath: url.path) {
      _ = try fileManager.replaceItemAt(url, withItemAt: tmp)
    } else {
      try fileManager.moveItem(at: tmp, to: url)
    }
  }

  private func pruneOrphans(in dir: URL, keeping: Set<String>) {
    let entries = (try? fileManager.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
    for url in entries where !keeping.contains(url.lastPathComponent) {
      try? fileManager.removeItem(at: url)
    }
  }

  private func loadPinOverrides() -> [String: PinOverrideEntry] {
    guard let url = pinOverridesURL,
          let data = try? Data(contentsOf: url),
          let file = try? JSONDecoder().decode(PinOverridesFile.self, from: data) else {
      return [:]
    }
    return file.overrides
  }

  private func savePinOverrides(_ overrides: [String: PinOverrideEntry]) {
    guard let root = rootURL, let url = pinOverridesURL else { return }
    try? fileManager.createDirectory(at: root, withIntermediateDirectories: true)
    let file = PinOverridesFile(overrides: overrides)
    if let data = try? JSONEncoder().encode(file) {
      try? data.write(to: url, options: [.atomic])
    }
  }
}
