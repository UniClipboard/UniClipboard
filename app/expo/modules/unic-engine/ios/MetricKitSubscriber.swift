import CommonCrypto
import Foundation
import MetricKit
import UnicCore

/// Subscribes to iOS MXMetricManager 24-hour summaries in the main app only,
/// extracting strictly sanitized numeric totals without device identifiers,
/// binding consent generation synchronously on didReceive entry, persisting
/// atomically before Go admission (Decision B1), recording SHA-256 content IDs
/// on admission before pruning pending reports, and clearing pending on disable.
final class MetricKitSubscriber: NSObject, MXMetricManagerSubscriber, @unchecked Sendable {
  private struct PendingReport {
    let key: String
    let entryGen: UInt64
    let report: [String: Any]
  }

  private let consentLock = NSLock()
  private var telemetryEnabled: Bool = false
  private var telemetryGeneration: UInt64 = 1

  private let stateQueue = DispatchQueue(label: "app.uniclipboard.metrickit")
  private var engineQueue: DispatchQueue?
  private weak var activeEngine: MobileEngine?
  private var engineGeneration: UInt64 = 0
  private var cacheDirPath: String?
  private var registered = false
  private var seenIDs: [String] = []

  private static let maxPendingReports = 16
  private static let maxSeenIDs = 64
  private static let pendingFilename = "pending-metrickit.json"
  private static let seenIDsFilename = "seen-metrickit-ids.json"

  func attach(engine: MobileEngine, queue: DispatchQueue, cacheDir: String, telemetryEnabled: Bool) {
    let curGen = syncConsentState(telemetryEnabled)
    stateQueue.sync {
      self.engineGeneration &+= 1
      self.activeEngine = engine
      self.engineQueue = queue
      self.cacheDirPath = cacheDir
      self.seenIDs = self.loadSeenIDsLocked(cacheDir: cacheDir)
      if !telemetryEnabled {
        self.clearPendingFileLocked()
        self.unregisterLocked()
        return
      }
      self.registerLocked()
      self.schedulePendingFlushLocked(expectedTelemetryGeneration: curGen)
    }
  }

  func detachEngine() {
    stateQueue.sync {
      self.engineGeneration &+= 1
      self.activeEngine = nil
    }
  }

  func detachAndUnregister() {
    stateQueue.sync {
      self.engineGeneration &+= 1
      self.activeEngine = nil
      self.engineQueue = nil
      self.unregisterLocked()
    }
  }

  func setTelemetryEnabled(_ enabled: Bool) {
    let (changed, curGen) = updateConsentState(enabled)
    stateQueue.sync {
      if !enabled {
        self.clearPendingFileLocked()
        self.unregisterLocked()
        return
      }
      if self.activeEngine != nil {
        self.registerLocked()
        if changed || self.hasPendingReportsLocked() {
          self.schedulePendingFlushLocked(expectedTelemetryGeneration: curGen)
        }
      }
    }
  }

  func didReceive(_ payloads: [MXMetricPayload]) {
    consentLock.lock()
    let entryEnabled = telemetryEnabled
    let entryGen = telemetryGeneration
    consentLock.unlock()
    guard entryEnabled else { return }

    let incoming = payloads.compactMap { payload -> PendingReport? in
      guard isConsentValid(entryGen) else { return nil }
      return Self.sanitizePayload(payload, entryGen: entryGen)
    }
    guard !incoming.isEmpty, isConsentValid(entryGen) else { return }

    stateQueue.async { [weak self] in
      guard let self else { return }
      guard self.isConsentValid(entryGen) else {
        return
      }
      if let dir = self.cacheDirPath {
        let unseen = incoming.filter { $0.key.isEmpty || !self.seenIDs.contains($0.key) }
        guard !unseen.isEmpty else { return }
        self.appendPendingLocked(unseen, cacheDir: dir)
      }
      self.schedulePendingFlushLocked(expectedTelemetryGeneration: entryGen)
    }
  }

  private func syncConsentState(_ enabled: Bool) -> UInt64 {
    consentLock.lock()
    if telemetryEnabled != enabled {
      telemetryEnabled = enabled
      telemetryGeneration &+= 1
    }
    let gen = telemetryGeneration
    consentLock.unlock()
    return gen
  }

  private func updateConsentState(_ enabled: Bool) -> (Bool, UInt64) {
    consentLock.lock()
    let changed = (telemetryEnabled != enabled)
    if changed {
      telemetryEnabled = enabled
      telemetryGeneration &+= 1
    }
    let gen = telemetryGeneration
    consentLock.unlock()
    return (changed, gen)
  }

  private func isConsentValid(_ entryGen: UInt64) -> Bool {
    consentLock.lock()
    let ok = telemetryEnabled && telemetryGeneration == entryGen
    consentLock.unlock()
    return ok
  }

  private func currentConsentGenerationIfEnabled() -> UInt64? {
    consentLock.lock()
    let gen: UInt64? = telemetryEnabled ? telemetryGeneration : nil
    consentLock.unlock()
    return gen
  }

  private func registerLocked() {
    guard !registered else { return }
    MXMetricManager.shared.add(self)
    registered = true
  }

  private func unregisterLocked() {
    guard registered else { return }
    MXMetricManager.shared.remove(self)
    registered = false
  }

  private func hasPendingReportsLocked() -> Bool {
    guard let dir = cacheDirPath else { return false }
    return FileManager.default.fileExists(atPath: pendingURL(cacheDir: dir).path)
  }

  private func schedulePendingFlushLocked(expectedTelemetryGeneration: UInt64) {
    guard isConsentValid(expectedTelemetryGeneration),
          let eng = activeEngine,
          let q = engineQueue else { return }
    let expectedEngineGen = engineGeneration
    q.async { [weak self] in
      guard let self else { return }
      var toSubmit: [PendingReport] = []
      var alreadySeenKeys: [String] = []
      var allowed = false
      self.stateQueue.sync {
        guard self.isConsentValid(expectedTelemetryGeneration),
              self.engineGeneration == expectedEngineGen,
              self.activeEngine === eng,
              let dir = self.cacheDirPath else {
          return
        }
        allowed = true
        let pending = self.readPendingLocked(cacheDir: dir)
        for item in pending {
          if !item.key.isEmpty && self.seenIDs.contains(item.key) {
            alreadySeenKeys.append(item.key)
            continue
          }
          if toSubmit.contains(where: { !$0.key.isEmpty && $0.key == item.key }) {
            continue
          }
          toSubmit.append(item)
        }
        if !alreadySeenKeys.isEmpty {
          self.removePendingKeysLocked(alreadySeenKeys, cacheDir: dir)
        }
      }
      guard allowed, !toSubmit.isEmpty else { return }

      for item in toSubmit {
        guard self.isConsentValid(expectedTelemetryGeneration) else {
          return
        }
        let stillAttached: Bool = self.stateQueue.sync {
          self.engineGeneration == expectedEngineGen && self.activeEngine === eng
        }
        guard stillAttached else {
          return
        }
        guard let data = try? JSONSerialization.data(withJSONObject: item.report),
              let json = String(data: data, encoding: .utf8) else {
          self.prunePendingKeyIfConsentValid(item.key, entryGen: expectedTelemetryGeneration)
          continue
        }
        do {
          try eng.submitMetricKitPayload(json)
          // B1: record seen digest first, then prune from pending.json on Go admission success.
          self.stateQueue.sync {
            guard self.isConsentValid(expectedTelemetryGeneration),
                  let dir = self.cacheDirPath else { return }
            if !item.key.isEmpty {
              if self.recordSeenIDsLocked([item.key], cacheDir: dir) {
                self.removePendingKeysLocked([item.key], cacheDir: dir)
              }
            }
          }
        } catch {
          if Self.isPermanentInvalidPayloadError(error) {
            self.prunePendingKeyIfConsentValid(item.key, entryGen: expectedTelemetryGeneration)
          }
          // Transient unavailability (e.g. invalid_state) keeps item in pending-metrickit.json for recovery.
        }
      }
    }
  }

  private func prunePendingKeyIfConsentValid(_ key: String, entryGen: UInt64) {
    guard !key.isEmpty else { return }
    stateQueue.sync {
      guard isConsentValid(entryGen), let dir = cacheDirPath else { return }
      removePendingKeysLocked([key], cacheDir: dir)
    }
  }

  private func pendingURL(cacheDir: String) -> URL {
    URL(fileURLWithPath: cacheDir, isDirectory: true).appendingPathComponent(Self.pendingFilename)
  }

  private func seenIDsURL(cacheDir: String) -> URL {
    URL(fileURLWithPath: cacheDir, isDirectory: true).appendingPathComponent(Self.seenIDsFilename)
  }

  private func clearPendingFileLocked() {
    guard let dir = cacheDirPath else { return }
    try? FileManager.default.removeItem(at: pendingURL(cacheDir: dir))
  }

  private func readPendingLocked(cacheDir: String) -> [PendingReport] {
    let url = pendingURL(cacheDir: cacheDir)
    guard let data = try? Data(contentsOf: url),
          let arr = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
      return []
    }
    var out: [PendingReport] = []
    for entry in arr {
      let report = (entry["report"] as? [String: Any]) ?? entry
      let key = (entry["key"] as? String) ?? Self.canonicalHash(beginMs: 0, endMs: 0, appVersion: "", multiVersion: false, out: report)
      let gen = (entry["entry_gen"] as? NSNumber)?.uint64Value ?? 0
      out.append(PendingReport(key: key, entryGen: gen, report: report))
    }
    return out
  }

  private func appendPendingLocked(_ incoming: [PendingReport], cacheDir: String) {
    guard !incoming.isEmpty else { return }
    var existing = readPendingLocked(cacheDir: cacheDir)
    for item in incoming {
      if !item.key.isEmpty && existing.contains(where: { $0.key == item.key }) {
        continue
      }
      existing.append(item)
    }
    if existing.count > Self.maxPendingReports {
      existing.removeFirst(existing.count - Self.maxPendingReports)
    }
    writePendingLocked(existing, cacheDir: cacheDir)
  }

  private func removePendingKeysLocked(_ keys: [String], cacheDir: String) {
    guard !keys.isEmpty else { return }
    let dropSet = Set(keys)
    let remaining = readPendingLocked(cacheDir: cacheDir).filter { !dropSet.contains($0.key) }
    writePendingLocked(remaining, cacheDir: cacheDir)
  }

  private func writePendingLocked(_ items: [PendingReport], cacheDir: String) {
    let url = pendingURL(cacheDir: cacheDir)
    if items.isEmpty {
      try? FileManager.default.removeItem(at: url)
      return
    }
    let serialized: [[String: Any]] = items.map { item in
      var dict: [String: Any] = ["key": item.key, "report": item.report]
      if item.entryGen > 0 {
        dict["entry_gen"] = NSNumber(value: item.entryGen)
      }
      return dict
    }
    if let encoded = try? JSONSerialization.data(withJSONObject: serialized) {
      try? encoded.write(to: url, options: .atomic)
    }
  }

  private func loadSeenIDsLocked(cacheDir: String) -> [String] {
    let url = seenIDsURL(cacheDir: cacheDir)
    guard let data = try? Data(contentsOf: url),
          let arr = try? JSONSerialization.jsonObject(with: data) as? [String] else {
      return []
    }
    return Array(arr.suffix(Self.maxSeenIDs))
  }

  @discardableResult
  private func recordSeenIDsLocked(_ keys: [String], cacheDir: String) -> Bool {
    var nextSeen = seenIDs
    var added = false
    for key in keys where !key.isEmpty && !nextSeen.contains(key) {
      nextSeen.append(key)
      added = true
    }
    guard added else { return true }
    if nextSeen.count > Self.maxSeenIDs {
      nextSeen.removeFirst(nextSeen.count - Self.maxSeenIDs)
    }
    guard let data = try? JSONSerialization.data(withJSONObject: nextSeen) else {
      return false
    }
    do {
      try data.write(to: seenIDsURL(cacheDir: cacheDir), options: .atomic)
      seenIDs = nextSeen
      return true
    } catch {
      return false
    }
  }

  private static func isPermanentInvalidPayloadError(_ error: Error) -> Bool {
    let candidates = [
      error.localizedDescription,
      (error as NSError).localizedFailureReason ?? "",
      String(describing: error),
    ]
    for text in candidates where !text.isEmpty {
      if let start = text.firstIndex(of: "{"),
         let end = text.lastIndex(of: "}"),
         start <= end,
         let data = String(text[start...end]).data(using: .utf8),
         let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
         let code = obj["code"] as? String {
        return code == "invalid_argument"
      }
      if text.contains(#""code":"invalid_argument""#) || text.lowercased().contains("invalid_argument") {
        return true
      }
    }
    return false
  }

  private static func sanitizePayload(_ payload: MXMetricPayload, entryGen: UInt64) -> PendingReport? {
    var out: [String: Any] = [:]
    if let cpu = payload.cpuMetrics {
      let sec = cpu.cumulativeCPUTime.converted(to: .seconds).value
      if sec.isFinite && sec >= 0 {
        out["cpu_seconds"] = sec
      }
    }
    if let net = payload.networkTransferMetrics {
      let wifiTx = Int64(max(0, net.cumulativeWifiUpload.converted(to: .bytes).value.rounded()))
      let wifiRx = Int64(max(0, net.cumulativeWifiDownload.converted(to: .bytes).value.rounded()))
      let cellTx = Int64(max(0, net.cumulativeCellularUpload.converted(to: .bytes).value.rounded()))
      let cellRx = Int64(max(0, net.cumulativeCellularDownload.converted(to: .bytes).value.rounded()))
      out["wifi_tx_bytes"] = wifiTx
      out["wifi_rx_bytes"] = wifiRx
      out["cellular_tx_bytes"] = cellTx
      out["cellular_rx_bytes"] = cellRx
    }
    if let mem = payload.memoryMetrics {
      let peak = UInt64(max(0, mem.peakMemoryUsage.converted(to: .bytes).value.rounded()))
      if peak > 0 {
        out["peak_memory_bytes"] = peak
      }
    }
    if let appTime = payload.applicationTimeMetrics {
      let fg = appTime.cumulativeForegroundTime.converted(to: .seconds).value
      let bg = appTime.cumulativeBackgroundTime.converted(to: .seconds).value
      if fg.isFinite && fg >= 0 {
        out["foreground_seconds"] = fg
      }
      if bg.isFinite && bg >= 0 {
        out["background_seconds"] = bg
      }
    }
    guard !out.isEmpty else { return nil }
    let beginMs = Int64((payload.timeStampBegin.timeIntervalSince1970 * 1000.0).rounded())
    let endMs = Int64((payload.timeStampEnd.timeIntervalSince1970 * 1000.0).rounded())
    var appVersion = ""
    var multiVersion = false
    if let obj = (payload as Any) as? NSObject {
      if obj.responds(to: NSSelectorFromString("latestApplicationVersion")),
         let v = obj.value(forKey: "latestApplicationVersion") as? String {
        appVersion = v
      }
      if obj.responds(to: NSSelectorFromString("includesMultipleApplicationVersions")),
         let b = obj.value(forKey: "includesMultipleApplicationVersions") as? Bool {
        multiVersion = b
      }
    }
    let key = canonicalHash(beginMs: beginMs, endMs: endMs, appVersion: appVersion, multiVersion: multiVersion, out: out)
    return PendingReport(key: key, entryGen: entryGen, report: out)
  }

  private static func canonicalHash(beginMs: Int64, endMs: Int64, appVersion: String, multiVersion: Bool, out: [String: Any]) -> String {
    let parts = out.keys.sorted().map { k -> String in
      if let d = out[k] as? Double {
        return "\(k)=d:\(d.bitPattern)"
      }
      if let n = out[k] as? NSNumber {
        let typeChar = String(cString: n.objCType)
        if typeChar == "d" || typeChar == "f" {
          return "\(k)=d:\(n.doubleValue.bitPattern)"
        }
        return "\(k)=i:\(n.int64Value)"
      }
      return "\(k)=\(String(describing: out[k]))"
    }.joined(separator: "&")
    let raw = "v1|\(beginMs)|\(endMs)|\(appVersion)|\(multiVersion ? "1" : "0")|\(parts)"
    return sha256Hex(raw)
  }

  private static func sha256Hex(_ input: String) -> String {
    let data = Data(input.utf8)
    var digest = [UInt8](repeating: 0, count: Int(CC_SHA256_DIGEST_LENGTH))
    data.withUnsafeBytes { buf in
      _ = CC_SHA256(buf.baseAddress, CC_LONG(data.count), &digest)
    }
    return digest.map { String(format: "%02x", $0) }.joined()
  }
}
