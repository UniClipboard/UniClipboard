import Foundation
import ImageIO
import UIKit
import UnicCore
import UniformTypeIdentifiers

/// Bridges the main app's single in-process Go Engine to the iOS Keyboard Extension
/// via a bounded read-only snapshot directory inside the shared App Group container.
///
/// The Keyboard Extension never links or starts Go Engine (0031 invariant). It reads
/// `snapshot.json` and downscaled thumbnails, and queues lightweight user intents
/// (such as toggling `pinned`) under `actions/` for the main app to reconcile.
final class KeyboardBridge {
  private static let maxItems = 50
  private static let maxImages = 20
  private static let maxImageBytes: Int64 = 4 * 1024 * 1024
  private static let thumbnailMaxPixel = 260
  private static let maxInsertTextBytes = 64 * 1024
  private static let maxFullTextChars = 8_000
  private static let maxTitleChars = 320

  struct Snapshot: Codable {
    let version: Int
    let generatedAtMs: Int64
    let historyRevision: String
    let selfDeviceName: String
    let items: [SnapshotItem]

    enum CodingKeys: String, CodingKey {
      case version
      case generatedAtMs = "generated_at_ms"
      case historyRevision = "history_revision"
      case selfDeviceName = "self_device_name"
      case items
    }
  }

  struct SnapshotItem: Codable {
    let id: String
    let revision: String
    let kind: String // "otp" | "url" | "image" | "text"
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

  struct Preferences: Codable {
    var soundEnabled: Bool
    var hapticEnabled: Bool
    var updatedAtMs: Int64

    enum CodingKeys: String, CodingKey {
      case soundEnabled = "sound_enabled"
      case hapticEnabled = "haptic_enabled"
      case updatedAtMs = "updated_at_ms"
    }
  }

  struct Heartbeat: Codable {
    let hasFullAccess: Bool
    let lastSeenAtMs: Int64

    enum CodingKeys: String, CodingKey {
      case hasFullAccess = "has_full_access"
      case lastSeenAtMs = "last_seen_at_ms"
    }
  }

  struct PendingAction: Codable {
    let id: String
    let kind: String
    let historyID: String
    let pinned: Bool?
    let createdAtMs: Int64

    enum CodingKeys: String, CodingKey {
      case id, kind, pinned
      case historyID = "history_id"
      case createdAtMs = "created_at_ms"
    }
  }

  private let fileManager = FileManager.default

  private var mainBundleID: String? {
    Bundle.main.bundleIdentifier
  }

  private var rootURL: URL? {
    guard let bundleID = mainBundleID else { return nil }
    return fileManager
      .containerURL(forSecurityApplicationGroupIdentifier: "group.\(bundleID).incoming-share")?
      .appendingPathComponent("keyboard-bridge", isDirectory: true)
  }

  private var snapshotURL: URL? { rootURL?.appendingPathComponent("snapshot.json") }
  private var preferencesURL: URL? { rootURL?.appendingPathComponent("preferences.json") }
  private var heartbeatURL: URL? { rootURL?.appendingPathComponent("heartbeat.json") }
  private var thumbnailsDir: URL? { rootURL?.appendingPathComponent("thumbnails", isDirectory: true) }
  private var imagesDir: URL? { rootURL?.appendingPathComponent("images", isDirectory: true) }
  private var actionsDir: URL? { rootURL?.appendingPathComponent("actions", isDirectory: true) }

  private func ensureDirectories() throws -> URL {
    guard let root = rootURL, let thumbs = thumbnailsDir, let imgs = imagesDir, let actions = actionsDir else {
      throw NSError(domain: "app.uniclipboard.keyboard", code: -1, userInfo: [NSLocalizedDescriptionKey: "app group unavailable"])
    }
    let attrs: [FileAttributeKey: Any] = [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
    for dir in [root, thumbs, imgs, actions] {
      if !fileManager.fileExists(atPath: dir.path) {
        try fileManager.createDirectory(at: dir, withIntermediateDirectories: true, attributes: attrs)
      }
    }
    return root
  }

  // MARK: - Status & Preferences

  func statusJSON() -> String {
    let extensionID = mainBundleID.map { "\($0).Keyboard" } ?? ""
    let keyboards = (UserDefaults.standard.array(forKey: "AppleKeyboards") as? [String]) ?? []
    let added = !extensionID.isEmpty && keyboards.contains(extensionID)

    var hasFullAccessSeen = false
    var lastSeenAtMs: Int64 = 0
    if let hbURL = heartbeatURL,
       let data = try? Data(contentsOf: hbURL),
       let hb = try? JSONDecoder().decode(Heartbeat.self, from: data) {
      hasFullAccessSeen = hb.hasFullAccess
      lastSeenAtMs = hb.lastSeenAtMs
    }

    let prefs = loadPreferences()
    var payload: [String: Any] = [
      "added": added,
      "has_full_access_seen": hasFullAccessSeen,
      "sound_enabled": prefs.soundEnabled,
      "haptic_enabled": prefs.hapticEnabled,
      "extension_bundle_id": extensionID,
    ]
    if lastSeenAtMs > 0 {
      payload["last_seen_at_ms"] = lastSeenAtMs
    }
    if let data = try? JSONSerialization.data(withJSONObject: payload),
       let text = String(data: data, encoding: .utf8) {
      return text
    }
    return "{\"added\":false,\"has_full_access_seen\":false,\"sound_enabled\":true,\"haptic_enabled\":true}"
  }

  func loadPreferences() -> Preferences {
    if let url = preferencesURL,
       let data = try? Data(contentsOf: url),
       let prefs = try? JSONDecoder().decode(Preferences.self, from: data) {
      return prefs
    }
    return Preferences(soundEnabled: true, hapticEnabled: true, updatedAtMs: 0)
  }

  func savePreferences(soundEnabled: Bool, hapticEnabled: Bool) throws {
    _ = try ensureDirectories()
    guard let url = preferencesURL else { return }
    let prefs = Preferences(
      soundEnabled: soundEnabled,
      hapticEnabled: hapticEnabled,
      updatedAtMs: Int64(Date().timeIntervalSince1970 * 1000)
    )
    let data = try JSONEncoder().encode(prefs)
    try writeAtomic(data: data, to: url)
  }

  // MARK: - Pending Keyboard Actions Reconciliation

  @discardableResult
  func drainPendingActions(engine: MobileEngine) -> Int {
    guard let actions = actionsDir else { return 0 }
    let files = (try? fileManager.contentsOfDirectory(
      at: actions,
      includingPropertiesForKeys: [.creationDateKey],
      options: [.skipsHiddenFiles]
    )) ?? []
    guard !files.isEmpty else { return 0 }

    let sorted = files.filter { $0.pathExtension == "json" }.sorted { $0.lastPathComponent < $1.lastPathComponent }
    var applied = 0
    for file in sorted {
      defer { try? fileManager.removeItem(at: file) }
      guard let data = try? Data(contentsOf: file),
            let action = try? JSONDecoder().decode(PendingAction.self, from: data),
            action.kind == "set_pin",
            let targetPinned = action.pinned else { continue }

      var itemErr: NSError?
      let itemJSON = engine.getHistoryItem(action.historyID, error: &itemErr)
      guard itemErr == nil,
            let itemData = itemJSON.data(using: .utf8),
            let itemObj = try? JSONSerialization.jsonObject(with: itemData) as? [String: Any],
            let rev = itemObj["revision"] else { continue }

      var tags = (itemObj["tags"] as? [String]) ?? []
      if targetPinned {
        if !tags.contains("pinned") { tags.append("pinned") }
      } else {
        tags.removeAll { $0 == "pinned" }
      }

      let updateReq: [String: Any] = [
        "id": action.historyID,
        "expected_revision": rev,
        "tags": tags,
      ]
      guard let reqData = try? JSONSerialization.data(withJSONObject: updateReq),
            let reqStr = String(data: reqData, encoding: .utf8) else { continue }
      var updateErr: NSError?
      _ = engine.updateHistoryItem(reqStr, error: &updateErr)
      if updateErr == nil {
        applied += 1
      }
    }
    return applied
  }

  // MARK: - Snapshot Publisher

  @discardableResult
  func publishSnapshot(engine: MobileEngine) throws -> Int {
    _ = try ensureDirectories()
    guard let snapURL = snapshotURL, let thumbsDir = thumbnailsDir, let imgsDir = imagesDir else {
      return 0
    }

    _ = drainPendingActions(engine: engine)

    // 1. Resolve self device name and remote device name map.
    var selfDeviceName = "iPhone"
    var deviceNames: [String: String] = [:]

    var stateErr: NSError?
    let stateJSON = engine.getState(&stateErr)
    if stateErr == nil,
       let data = stateJSON.data(using: .utf8),
       let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
      if let selfObj = obj["self"] as? [String: Any],
         let name = selfObj["name"] as? String, !name.isEmpty {
        selfDeviceName = name
      }
      if let devices = obj["devices"] as? [[String: Any]] {
        for d in devices {
          if let id = d["id"] as? String {
            let name = (d["name"] as? String).flatMap { $0.isEmpty ? nil : $0 }
              ?? (d["reported_name"] as? String).flatMap { $0.isEmpty ? nil : $0 }
              ?? String(id.prefix(8))
            deviceNames[id] = name
          }
        }
      }
    }

    // 2. Fetch recent items + pinned items (deduplicated, preserving recency order).
    let recentReq = "{\"limit\":\(Self.maxItems),\"filter\":{\"kinds\":[\"text\",\"image\"]}}"
    var listErr: NSError?
    let recentJSON = engine.listHistory(recentReq, error: &listErr)
    if let listErr = listErr { throw listErr }

    guard let recentData = recentJSON.data(using: .utf8),
          let recentObj = try? JSONSerialization.jsonObject(with: recentData) as? [String: Any] else {
      return 0
    }
    let historyRevision = (recentObj["history_revision"] as? String) ?? "0"
    let recentItems = (recentObj["items"] as? [[String: Any]]) ?? []

    let pinnedReq = "{\"limit\":25,\"filter\":{\"kinds\":[\"text\",\"image\"],\"tags\":[\"pinned\"]}}"
    var pinErr: NSError?
    let pinnedJSON = engine.listHistory(pinnedReq, error: &pinErr)
    let pinnedItems: [[String: Any]] = {
      guard pinErr == nil,
            let d = pinnedJSON.data(using: .utf8),
            let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any],
            let arr = o["items"] as? [[String: Any]] else { return [] }
      return arr
    }()

    var seen = Set<String>()
    var merged: [[String: Any]] = []
    for raw in recentItems {
      if let id = raw["id"] as? String, !seen.contains(id) {
        seen.insert(id)
        merged.append(raw)
      }
    }
    for raw in pinnedItems where merged.count < Self.maxItems + 15 {
      if let id = raw["id"] as? String, !seen.contains(id) {
        seen.insert(id)
        merged.append(raw)
      }
    }

    // 3. Build SnapshotItems with bounded text & downscaled thumbnails.
    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
    var items: [SnapshotItem] = []
    var keepThumbs = Set<String>()
    var keepImages = Set<String>()
    var imageCount = 0

    for raw in merged {
      guard items.count < Self.maxItems,
            let id = raw["id"] as? String,
            let kind = raw["kind"] as? String else { continue }

      // Skip expired items.
      if let expiresAt = (raw["expires_at_ms"] as? NSNumber)?.int64Value, expiresAt > 0, expiresAt < nowMs {
        continue
      }

      let revision = String(describing: raw["revision"] ?? "1")
      let createdAtMs = (raw["created_at_ms"] as? NSNumber)?.int64Value ?? nowMs
      let tags = (raw["tags"] as? [String]) ?? []
      let pinned = tags.contains("pinned")
      let deviceName = resolveDeviceName(raw: raw, deviceNames: deviceNames, selfDeviceName: selfDeviceName)

      if kind == "image" {
        guard imageCount < Self.maxImages else { continue }
        var expErr: NSError?
        let expJSON = engine.exportHistoryImagePreview(id, error: &expErr)
        guard expErr == nil,
              let expData = expJSON.data(using: .utf8),
              let expObj = try? JSONSerialization.jsonObject(with: expData) as? [String: Any],
              let srcPath = expObj["path"] as? String else { continue }

        let srcURL = URL(fileURLWithPath: srcPath)
        let partObj = expObj["part"] as? [String: Any]
        let mime = (partObj?["mime"] as? String) ?? "image/png"
        let rawName = (partObj?["name"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        let safeID = safeFileStem(id)
        let ext = imageExtension(for: mime, fallbackName: rawName)
        let displayName = rawName ?? "图片.\(ext)"

        let thumbName = "\(safeID).jpg"
        let thumbDest = thumbsDir.appendingPathComponent(thumbName)
        let dims = generateThumbnailIfNeeded(from: srcURL, to: thumbDest, maxPixel: Self.thumbnailMaxPixel)
        guard fileManager.fileExists(atPath: thumbDest.path) else { continue }
        keepThumbs.insert(thumbName)

        let imgName = "\(safeID).\(ext)"
        let imgDest = imgsDir.appendingPathComponent(imgName)
        var copiedImageName: String? = nil
        if let sizeVal = (try? srcURL.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
           Int64(sizeVal) <= Self.maxImageBytes {
          if !fileManager.fileExists(atPath: imgDest.path) {
            try? fileManager.copyItem(at: srcURL, to: imgDest)
            try? fileManager.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: imgDest.path)
          }
          if fileManager.fileExists(atPath: imgDest.path) {
            copiedImageName = imgName
            keepImages.insert(imgName)
          }
        }

        let formatLabel = ext.uppercased()
        let metaText: String
        if let dims {
          metaText = "\(dims.width) × \(dims.height) · \(formatLabel)"
        } else {
          metaText = formatLabel
        }

        items.append(SnapshotItem(
          id: id,
          revision: revision,
          kind: "image",
          title: displayName,
          insertText: nil,
          fullText: nil,
          subtitle: nil,
          domain: nil,
          imageMeta: metaText,
          thumbnailFile: thumbName,
          imageFile: copiedImageName,
          deviceName: deviceName,
          createdAtMs: createdAtMs,
          pinned: pinned
        ))
        imageCount += 1
        continue
      }

      // Text / HTML / Mixed with text representation
      var textErr: NSError?
      let prevJSON = engine.readHistoryTextPreview(id, maxBytes: Self.maxInsertTextBytes, error: &textErr)
      guard textErr == nil,
            let prevData = prevJSON.data(using: .utf8),
            let prevObj = try? JSONSerialization.jsonObject(with: prevData) as? [String: Any],
            let rawText = prevObj["text"] as? String else { continue }

      let trimmed = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !trimmed.isEmpty else { continue }
      // Never expose pairing tickets or secrets in the keyboard snapshot.
      if trimmed.hasPrefix("unic-pair:") { continue }

      let classified = classifyText(rawText: rawText, trimmed: trimmed)
      items.append(SnapshotItem(
        id: id,
        revision: revision,
        kind: classified.kind,
        title: classified.title,
        insertText: classified.insertText,
        fullText: String(rawText.prefix(Self.maxFullTextChars)),
        subtitle: classified.subtitle,
        domain: classified.domain,
        imageMeta: nil,
        thumbnailFile: nil,
        imageFile: nil,
        deviceName: deviceName,
        createdAtMs: createdAtMs,
        pinned: pinned
      ))
    }

    // 4. Write snapshot atomically and prune orphaned media files.
    let snapshot = Snapshot(
      version: 1,
      generatedAtMs: nowMs,
      historyRevision: historyRevision,
      selfDeviceName: selfDeviceName,
      items: items
    )
    let encoded = try JSONEncoder().encode(snapshot)
    try writeAtomic(data: encoded, to: snapURL)
    pruneOrphans(in: thumbsDir, keeping: keepThumbs)
    pruneOrphans(in: imgsDir, keeping: keepImages)
    return items.count
  }

  // MARK: - Classification & Media Helpers

  private struct ClassifiedText {
    let kind: String
    let title: String
    let insertText: String
    let subtitle: String?
    let domain: String?
  }

  private func classifyText(rawText: String, trimmed: String) -> ClassifiedText {
    // 1. High-precision OTP detection
    if let otp = extractOTP(from: trimmed) {
      let hint: String
      if trimmed == otp.code || trimmed.replacingOccurrences(of: " ", with: "") == otp.code {
        hint = "验证码 · 点按填入"
      } else {
        hint = otp.hint
      }
      return ClassifiedText(
        kind: "otp",
        title: otp.code,
        insertText: otp.code,
        subtitle: hint,
        domain: nil
      )
    }

    // 2. Single-line HTTP(S) URL detection
    if !trimmed.contains("\n"),
       (trimmed.hasPrefix("http://") || trimmed.hasPrefix("https://")),
       let url = URL(string: trimmed),
       let host = url.host, !host.isEmpty {
      let cleanDomain = host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
      let pathSummary = url.path.isEmpty || url.path == "/" ? cleanDomain : "\(cleanDomain)\(url.path)"
      return ClassifiedText(
        kind: "url",
        title: String(pathSummary.prefix(Self.maxTitleChars)),
        insertText: trimmed,
        subtitle: trimmed,
        domain: cleanDomain
      )
    }

    // 3. Plain text
    let compactTitle = trimmed
      .replacingOccurrences(of: "\r\n", with: "\n")
      .prefix(Self.maxTitleChars)
    return ClassifiedText(
      kind: "text",
      title: String(compactTitle),
      insertText: rawText,
      subtitle: nil,
      domain: nil
    )
  }

  private func extractOTP(from trimmed: String) -> (code: String, hint: String)? {
    guard trimmed.count <= 240 else { return nil }

    // Pure 6-digit code (or 3+3 spaced/hyphenated)
    let stripped = trimmed.replacingOccurrences(of: " ", with: "").replacingOccurrences(of: "-", with: "")
    if stripped.count == 6, stripped.allSatisfy({ $0.isASCII && $0.isNumber }), trimmed.count <= 7 {
      return (stripped, "6 位验证码")
    }

    // Contextual OTP message with explicit keywords
    let lower = trimmed.lowercased()
    let keywords = [
      "验证码", "认证码", "校验码", "动态码", "安全码", "取件码", "确认码",
      "verification code", "security code", "passcode", "one-time",
      "otp", "2fa", "auth code", "login code", "sign-in code", "signin code"
    ]
    guard keywords.contains(where: { lower.contains($0) }) else { return nil }

    let pattern = "(?<![0-9A-Za-z])([0-9]{4,8})(?![0-9A-Za-z])"
    guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }
    let range = NSRange(trimmed.startIndex..., in: trimmed)
    let matches = regex.matches(in: trimmed, range: range)
    // Pick the first 6-digit match if available, otherwise first 4-8 digit match.
    var chosen: String?
    for match in matches {
      guard let r = Range(match.range(at: 1), in: trimmed) else { continue }
      let candidate = String(trimmed[r])
      if candidate.count == 6 {
        chosen = candidate
        break
      }
      if chosen == nil {
        chosen = candidate
      }
    }
    guard let code = chosen else { return nil }

    // Extract optional bracketed sender e.g. 【GitHub】 or [Apple]
    var senderPrefix = ""
    if let open = trimmed.firstIndex(of: "【"), let close = trimmed.firstIndex(of: "】"), open < close {
      let inner = trimmed[trimmed.index(after: open)..<close]
      if !inner.isEmpty && inner.count <= 24 {
        senderPrefix = "\(inner) "
      }
    }
    return (code, "\(senderPrefix)验证码 · 自动提取")
  }

  private func resolveDeviceName(raw: [String: Any], deviceNames: [String: String], selfDeviceName: String) -> String {
    if let origin = raw["origin"] as? [String: Any],
       let originKind = origin["kind"] as? String {
      if originKind == "remote_device", let devID = origin["device_id"] as? String {
        return deviceNames[devID] ?? String(devID.prefix(8))
      }
      if originKind == "external" {
        return "外部渠道"
      }
      if originKind == "migration" {
        return "v1 迁移"
      }
    }
    if let srcApp = raw["source_application"] as? [String: Any],
       let appName = srcApp["name"] as? String, !appName.isEmpty {
      return appName
    }
    return selfDeviceName
  }

  private func safeFileStem(_ id: String) -> String {
    let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
    let cleaned = id.unicodeScalars.map { allowed.contains($0) ? Character($0) : "_" }
    return String(cleaned.prefix(80))
  }

  private func imageExtension(for mime: String, fallbackName: String?) -> String {
    switch mime.lowercased() {
    case "image/jpeg", "image/jpg": return "jpg"
    case "image/heic": return "heic"
    case "image/webp": return "webp"
    case "image/gif": return "gif"
    case "image/png": return "png"
    default:
      if let name = fallbackName {
        let ext = URL(fileURLWithPath: name).pathExtension.lowercased()
        if !ext.isEmpty && ext.count <= 5 { return ext }
      }
      return "png"
    }
  }

  private func generateThumbnailIfNeeded(from srcURL: URL, to destURL: URL, maxPixel: Int) -> (width: Int, height: Int)? {
    guard let source = CGImageSourceCreateWithURL(srcURL as CFURL, nil) else { return nil }
    var dims: (width: Int, height: Int)?
    if let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
       let w = props[kCGImagePropertyPixelWidth] as? Int,
       let h = props[kCGImagePropertyPixelHeight] as? Int,
       w > 0, h > 0 {
      dims = (w, h)
    }

    if !fileManager.fileExists(atPath: destURL.path) {
      let options: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceThumbnailMaxPixelSize: maxPixel,
        kCGImageSourceShouldCacheImmediately: true,
      ]
      if let cgThumb = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) {
        let uiImg = UIImage(cgImage: cgThumb)
        if let jpeg = uiImg.jpegData(compressionQuality: 0.78) {
          try? writeAtomic(data: jpeg, to: destURL)
        }
      }
    }
    return dims
  }

  private func writeAtomic(data: Data, to url: URL) throws {
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
}
