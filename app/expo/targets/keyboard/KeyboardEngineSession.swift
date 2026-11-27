import Foundation
import ImageIO
import Network
import UIKit
import UnicCore

private final class KeyboardEngineEventBridge: NSObject, MobileEventSinkProtocol, @unchecked Sendable {
  private let onEventCb: (String) -> Void
  private let onResyncCb: (String) -> Void

  init(onEvent: @escaping (String) -> Void, onResync: @escaping (String) -> Void) {
    self.onEventCb = onEvent
    self.onResyncCb = onResync
  }

  func onEvent(_ eventJSON: String?) {
    if let json = eventJSON {
      onEventCb(json)
    }
  }

  func onResyncRequired(_ reason: String?) {
    onResyncCb(reason ?? "overflow")
  }
}

/// Manages the iOS Keyboard Extension's time-sliced Go Engine instance (0035):
/// - Opens the shared App Group DataDir with memory-constrained GC (`GOGC=25, GOMEMLIMIT=18MB`)
///   and `extension_mode: true` (skipping external link preview fetches).
/// - Smoothly degrades to snapshot mode if the main app holds `unic.lock` in the foreground.
/// - Auto-captures system clipboard (text + bounded images) and flushes pre-connect captures
///   as soon as trusted peers transition to `connected`.
/// - Receives remote items in real time, refreshes the keyboard card list, and automatically
///   writes received content to `UIPasteboard.general` with cross-process anti-loopback.
final class KeyboardEngineSession {
  enum ConnectionState: Equatable {
    case idle
    case connecting
    case connected(peerCount: Int)
    case degraded
    case error
  }

  private struct PendingFlush {
    let historyID: String
    var sentDeviceIDs: Set<String>
  }

  private struct ActiveSendBatch {
    let historyID: String
    let clipTitle: String
    var operationIDs: [String]
    var deviceOrder: [String]
    var deviceStates: [String: KeyboardSendDeviceState]
    var lastHighlightText: String?
    var graceExpired: Bool
  }

  private static let maxItems = 50
  private static let maxImages = 20
  private static let maxTextBytes = 4 * 1024 * 1024
  private static let maxCaptureImageBytes = 4 * 1024 * 1024
  private static let maxReceiveImageBytes = 8 * 1024 * 1024
  private static let maxCaptureImagePixel: CGFloat = 2048
  private static let maxInsertTextBytes = 64 * 1024
  private static let maxFullTextChars = 8_000
  private static let maxTitleChars = 320
  private static let thumbnailMaxPixel = 260
  private static let maxPendingFlushItems = 10
  private static let sendOfflineGraceSeconds: TimeInterval = 5.5

  private static let configureMemoryOnce: Void = {
    MobileConfigureRuntimeMemory(25, 18)
  }()

  private let store: KeyboardSnapshotStore
  private let queue = DispatchQueue(label: "app.uniclipboard.keyboard.engine", qos: .userInitiated)
  private let liveLock = NSLock()
  private var _isLive = false
  private var engine: MobileEngine?
  private var pathMonitor: NWPathMonitor?
  private var refreshWorkItem: DispatchWorkItem?
  private var sendGraceWorkItem: DispatchWorkItem?
  private var autoSyncEnabled = true
  private var strings = KeyboardStrings()

  private var seenReceiptIDs = Set<String>()
  private var pendingFlushes: [PendingFlush] = []
  private var activeSendBatch: ActiveSendBatch?
  private var trustedDeviceOrder: [String] = []
  private var lastCheckedChangeCount = -1
  private var selfDeviceName = "iPhone"
  private var deviceNames: [String: String] = [:]
  private var lastHistoryRevision = "0"
  private var lastItems: [KeyboardClipItem] = []
  private var lastKeepThumbs = Set<String>()
  private var lastKeepImages = Set<String>()
  private var activeGeneration: UInt64 = 0

  var onStateChanged: (@MainActor (ConnectionState) -> Void)?
  var onItemsUpdated: (@MainActor ([KeyboardClipItem]) -> Void)?
  var onToast: (@MainActor (String) -> Void)?
  var onSendBatchUpdated: (@MainActor (KeyboardSendBatchState?) -> Void)?

  init(store: KeyboardSnapshotStore) {
    self.store = store
  }

  var isLive: Bool {
    liveLock.lock()
    defer { liveLock.unlock() }
    return _isLive
  }

  private func setLive(_ value: Bool) {
    liveLock.lock()
    _isLive = value
    liveLock.unlock()
  }

  func start(autoSyncEnabled: Bool, strings: KeyboardStrings) {
    queue.async { [weak self] in
      guard let self else { return }
      self.autoSyncEnabled = autoSyncEnabled
      self.strings = strings
      if self.engine != nil {
        if autoSyncEnabled {
          self.captureSystemClipboardIfNeeded()
        }
        return
      }

      self.activeGeneration &+= 1
      let gen = self.activeGeneration

      guard let dataDir = self.store.sharedEngineDataDir,
            let cacheDir = self.store.sharedEngineCacheDir else {
        self.emitState(.degraded)
        return
      }

      // Do not initialize a brand-new database inside the keyboard extension before
      // the main app completes first-install setup / v1 migration.
      let dbURL = dataDir.appendingPathComponent("unic.db")
      guard FileManager.default.fileExists(atPath: dbURL.path) else {
        self.emitState(.degraded)
        return
      }

      _ = Self.configureMemoryOnce
      let attrs: [FileAttributeKey: Any] = [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication]
      try? FileManager.default.createDirectory(at: cacheDir, withIntermediateDirectories: true, attributes: attrs)

      let deviceName: String = DispatchQueue.main.sync { UIDevice.current.name }
      let cfg: [String: Any] = [
        "data_dir": dataDir.resolvingSymlinksInPath().path,
        "cache_dir": cacheDir.resolvingSymlinksInPath().path,
        "device_name": deviceName,
        "extension_mode": true,
        "host_capabilities": [
          "read_formats": ["text/plain", "text/html", "image/png"],
          "write_formats": ["text/plain", "text/html", "image/png"],
          "read_files": false,
          "write_files": false,
          "observe": "available",
          "apply": "available",
        ],
      ]
      guard let cfgData = try? JSONSerialization.data(withJSONObject: cfg),
            let cfgJSON = String(data: cfgData, encoding: .utf8) else {
        self.emitState(.error)
        return
      }

      // Short retry on `in_use` in case the main app just entered background and is finishing its ~10ms close().
      var created: MobileEngine?
      var sawInUse = false
      for attempt in 0..<4 {
        guard self.activeGeneration == gen else { return }
        var err: NSError?
        if let eng = MobileNewEngine(cfgJSON, &err) {
          created = eng
          break
        }
        let msg = err?.localizedDescription ?? ""
        if msg.contains("\"in_use\"") {
          sawInUse = true
          if attempt < 3 {
            Thread.sleep(forTimeInterval: 0.05)
            continue
          }
        }
        break
      }

      guard self.activeGeneration == gen else {
        try? created?.close(nil)
        return
      }

      guard let eng = created else {
        self.emitState(sawInUse ? .degraded : .error)
        return
      }

      // Snapshot already-existing receipts so we only auto-apply items arriving during this active session.
      self.seenReceiptIDs.removeAll()
      self.pendingFlushes.removeAll()
      self.activeSendBatch = nil
      self.sendGraceWorkItem?.cancel()
      self.sendGraceWorkItem = nil
      self.seedExistingReceipts(eng)

      let sink = KeyboardEngineEventBridge(
        onEvent: { [weak self] eventJSON in
          self?.handleEngineEvent(eventJSON, generation: gen)
        },
        onResync: { [weak self] _ in
          self?.queue.async { [weak self] in
            guard let self, self.activeGeneration == gen else { return }
            self.scheduleHistoryRefresh(delay: 0.1)
            self.refreshPeersAndFlush()
            self.pollActiveSendOperationsLocked()
          }
        }
      )

      var subErr: NSError?
      _ = eng.subscribe(sink, error: &subErr)
      if subErr != nil {
        try? eng.close(nil)
        self.emitState(.error)
        return
      }

      do {
        try eng.setRuntimeState("active")
        try eng.start(nil)
      } catch {
        try? eng.close(nil)
        self.emitState(.error)
        return
      }

      self.engine = eng
      self.setLive(true)
      self.startNetworkMonitor()
      self.emitState(.connecting)

      self.refreshHistoryLocked(eng, writeSnapshot: true)
      if self.autoSyncEnabled {
        self.captureSystemClipboardIfNeeded()
      }
      self.refreshPeersAndFlush()
    }
  }

  func stop() {
    queue.async { [weak self] in
      guard let self else { return }
      self.activeGeneration &+= 1
      self.refreshWorkItem?.cancel()
      self.refreshWorkItem = nil
      self.sendGraceWorkItem?.cancel()
      self.sendGraceWorkItem = nil
      self.stopNetworkMonitor()
      self.pendingFlushes.removeAll()
      self.activeSendBatch = nil
      self.emitSendBatch(nil)

      if let eng = self.engine {
        if !self.lastItems.isEmpty {
          self.store.saveSnapshot(
            items: self.lastItems,
            historyRevision: self.lastHistoryRevision,
            selfDeviceName: self.selfDeviceName,
            keepThumbs: self.lastKeepThumbs,
            keepImages: self.lastKeepImages
          )
        }
        try? eng.close(nil)
        self.engine = nil
        self.setLive(false)
      }
      self.emitState(.idle)
    }
  }

  func retrySend(historyID: String, deviceID: String) {
    queue.async { [weak self] in
      guard let self, let eng = self.engine else { return }
      guard var batch = self.activeSendBatch, batch.historyID == historyID else { return }

      try? eng.notifyNetworkChanged()
      if !batch.deviceOrder.contains(deviceID) {
        batch.deviceOrder.append(deviceID)
      }
      batch.deviceStates[deviceID] = .sending(progressPercent: nil)
      self.activeSendBatch = batch
      self.emitActiveSendBatchLocked()

      let req: [String: Any] = [
        "request_id": "ios-kb-retry-\(UUID().uuidString)",
        "history_id": historyID,
        "device_ids": [deviceID],
      ]
      guard let reqData = try? JSONSerialization.data(withJSONObject: req),
            let reqJSON = String(data: reqData, encoding: .utf8) else { return }

      var sendErr: NSError?
      let resJSON = eng.resendHistoryItem(nil, reqJSON: reqJSON, error: &sendErr)
      guard var updated = self.activeSendBatch, updated.historyID == historyID else { return }
      if sendErr != nil {
        updated.deviceStates[deviceID] = .failed
        self.activeSendBatch = updated
        self.emitActiveSendBatchLocked()
        return
      }

      var opID = ""
      var disposition = ""
      if let data = resJSON.data(using: .utf8),
         let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
        opID = (obj["operation_id"] as? String) ?? ""
        disposition = (obj["sync_disposition"] as? String) ?? ""
      }

      if !opID.isEmpty {
        if !updated.operationIDs.contains(opID) {
          updated.operationIDs.append(opID)
        }
        self.activeSendBatch = updated
        self.pollActiveSendOperationsLocked()
      } else if disposition == "no_targets" {
        updated.deviceStates[deviceID] = .offline
        self.activeSendBatch = updated
        self.emitActiveSendBatchLocked()
      } else {
        updated.deviceStates[deviceID] = .failed
        self.activeSendBatch = updated
        self.emitActiveSendBatchLocked()
      }
    }
  }

  func checkClipboardChange() {
    queue.async { [weak self] in
      guard let self, self.autoSyncEnabled, self.engine != nil else { return }
      self.captureSystemClipboardIfNeeded()
    }
  }

  func manualRefresh() {
    queue.async { [weak self] in
      guard let self, let eng = self.engine else { return }
      try? eng.notifyNetworkChanged()
      if self.autoSyncEnabled {
        self.captureSystemClipboardIfNeeded()
      }
      self.refreshHistoryLocked(eng, writeSnapshot: true)
      self.refreshPeersAndFlush()
      self.emitToast(self.strings.toastSynced)
    }
  }

  func togglePin(historyID: String, pinned: Bool, completion: @escaping @MainActor (Bool) -> Void) {
    queue.async { [weak self] in
      guard let self, let eng = self.engine else {
        Task { @MainActor in completion(false) }
        return
      }
      var itemErr: NSError?
      let itemJSON = eng.getHistoryItem(historyID, error: &itemErr)
      guard itemErr == nil,
            let data = itemJSON.data(using: .utf8),
            let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let rev = obj["revision"] else {
        Task { @MainActor in completion(false) }
        return
      }
      var tags = (obj["tags"] as? [String]) ?? []
      if pinned {
        if !tags.contains("pinned") { tags.append("pinned") }
      } else {
        tags.removeAll { $0 == "pinned" }
      }
      let updateReq: [String: Any] = [
        "id": historyID,
        "expected_revision": rev,
        "tags": tags,
      ]
      guard let reqData = try? JSONSerialization.data(withJSONObject: updateReq),
            let reqStr = String(data: reqData, encoding: .utf8) else {
        Task { @MainActor in completion(false) }
        return
      }
      var updateErr: NSError?
      _ = eng.updateHistoryItem(reqStr, error: &updateErr)
      let ok = (updateErr == nil)
      if ok {
        self.refreshHistoryLocked(eng, writeSnapshot: true)
      }
      Task { @MainActor in completion(ok) }
    }
  }

  func handleMemoryWarning() {
    queue.async {
      MobileFreeOSMemory()
    }
  }

  // MARK: - Network Monitor

  private func startNetworkMonitor() {
    guard pathMonitor == nil else { return }
    let monitor = NWPathMonitor()
    monitor.pathUpdateHandler = { [weak self] _ in
      guard let self else { return }
      self.queue.async {
        try? self.engine?.notifyNetworkChanged()
      }
    }
    monitor.start(queue: DispatchQueue(label: "app.uniclipboard.keyboard.netmon"))
    pathMonitor = monitor
  }

  private func stopNetworkMonitor() {
    pathMonitor?.cancel()
    pathMonitor = nil
  }

  // MARK: - System Clipboard Auto-Capture & Pre-Connect Flush

  private func captureSystemClipboardIfNeeded() {
    guard let eng = self.engine else { return }

    // Fast path: check changeCount first (never triggers iOS system paste prompt).
    let quickCC: Int = DispatchQueue.main.sync { UIPasteboard.general.changeCount }
    if quickCC == lastCheckedChangeCount || store.isChangeCountHandled(quickCC) {
      return
    }

    let pb: (Int, String?, UIImage?) = DispatchQueue.main.sync {
      let p = UIPasteboard.general
      let cc = p.changeCount
      if cc == self.lastCheckedChangeCount || self.store.isChangeCountHandled(cc) {
        return (cc, nil, nil)
      }
      if p.hasStrings {
        return (cc, p.string, nil)
      }
      if p.hasImages {
        return (cc, nil, p.image)
      }
      return (cc, nil, nil)
    }

    let changeCount = pb.0
    lastCheckedChangeCount = changeCount
    if store.isChangeCountHandled(changeCount) {
      return
    }

    var leaseID: String?
    var contentHash = ""
    var capturedTitle = ""

    if let text = pb.1 {
      let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !trimmed.isEmpty, !trimmed.hasPrefix("unic-pair:") else { return }
      guard text.utf8.count <= Self.maxTextBytes else { return }
      contentHash = KeyboardSnapshotStore.sha256Hex(ofText: text)
      if store.shouldSuppressCapture(changeCount: changeCount, contentSHA256: contentHash) {
        return
      }
      let singleLine = trimmed.replacingOccurrences(of: "\r\n", with: " ").replacingOccurrences(of: "\n", with: " ")
      capturedTitle = String(singleLine.prefix(28))
      var err: NSError?
      let hJSON = eng.importText(text, html: "", obsKey: "ios-kb:\(changeCount)", error: &err)
      if err == nil,
         let data = hJSON.data(using: .utf8),
         let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
        leaseID = obj["lease_id"] as? String
      }
    } else if let image = pb.2 {
      guard let pngData = encodeBoundedPNG(from: image) else {
        emitToast(strings.toastImageTooLarge)
        return
      }
      contentHash = KeyboardSnapshotStore.sha256Hex(of: pngData)
      if store.shouldSuppressCapture(changeCount: changeCount, contentSHA256: contentHash) {
        return
      }
      capturedTitle = strings.sendClipImageTitle
      // fileName MUST be empty so Engine stores as clipboard representation (not file attachment).
      var err: NSError?
      let hJSON = eng.importBytes("image/png", fileName: "", data: pngData, error: &err)
      if err == nil,
         let data = hJSON.data(using: .utf8),
         let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
        leaseID = obj["lease_id"] as? String
      }
    } else {
      return
    }

    guard let lid = leaseID else { return }
    defer { try? eng.releaseContent(lid) }

    let reqObj: [String: Any] = [
      "request_id": "ios-kb-cap-\(UUID().uuidString)",
      "lease_id": lid,
    ]
    guard let reqData = try? JSONSerialization.data(withJSONObject: reqObj),
          let reqJSON = String(data: reqData, encoding: .utf8) else { return }

    var commitErr: NSError?
    let subJSON = eng.commitContent(nil, reqJSON: reqJSON, error: &commitErr)
    guard commitErr == nil else { return }

    store.recordPasteboardState(changeCount: changeCount, contentSHA256: contentHash, origin: "captured")

    var historyID = ""
    var syncDisposition = ""
    var operationID = ""
    if let subData = subJSON.data(using: .utf8),
       let subObj = try? JSONSerialization.jsonObject(with: subData) as? [String: Any] {
      historyID = (subObj["history_id"] as? String) ?? ""
      syncDisposition = (subObj["sync_disposition"] as? String) ?? ""
      operationID = (subObj["operation_id"] as? String) ?? ""
    }

    if !historyID.isEmpty && (syncDisposition == "no_targets" || syncDisposition == "queued") {
      startActiveSendBatchLocked(
        historyID: historyID,
        clipTitle: capturedTitle,
        initialOperationID: operationID.isEmpty ? nil : operationID
      )
    }

    refreshHistoryLocked(eng, writeSnapshot: true)
    refreshPeersAndFlush()
  }

  private func encodeBoundedPNG(from image: UIImage) -> Data? {
    let pixelW = image.size.width * image.scale
    let pixelH = image.size.height * image.scale
    guard pixelW > 0, pixelH > 0 else { return nil }

    let maxDim = max(pixelW, pixelH)
    let candidateCaps: [CGFloat] = [Self.maxCaptureImagePixel, 1280, 960]
    for cap in candidateCaps {
      let scale = min(1.0, cap / maxDim)
      let size = CGSize(width: max(1, (pixelW * scale).rounded()), height: max(1, (pixelH * scale).rounded()))
      let format = UIGraphicsImageRendererFormat()
      format.scale = 1
      let data = UIGraphicsImageRenderer(size: size, format: format).pngData { _ in
        image.draw(in: CGRect(origin: .zero, size: size))
      }
      if !data.isEmpty && data.count <= Self.maxCaptureImageBytes {
        return data
      }
    }
    return nil
  }

  // MARK: - Events, Peer Flush & Live Receive -> System Pasteboard

  private func handleEngineEvent(_ eventJSON: String, generation: UInt64) {
    queue.async { [weak self] in
      guard let self, self.activeGeneration == generation, self.engine != nil else { return }
      if eventJSON.contains("\"devices\"") {
        self.refreshPeersAndFlush()
      }
      if eventJSON.contains("\"operations\"") {
        self.pollActiveSendOperationsLocked()
      }
      if eventJSON.contains("\"received\"") {
        self.handleIncomingReceivedLocked()
      }
      if eventJSON.contains("\"history\"") || eventJSON.contains("\"received\"") || eventJSON.contains("\"devices\"") {
        self.scheduleHistoryRefresh(delay: 0.12)
      }
    }
  }

  private func startActiveSendBatchLocked(
    historyID: String,
    clipTitle: String,
    initialOperationID: String?
  ) {
    var order = trustedDeviceOrder
    var states: [String: KeyboardSendDeviceState] = [:]
    for devID in order {
      states[devID] = .waiting
    }
    var opIDs: [String] = []
    var alreadySent = Set<String>()

    if let opID = initialOperationID, !opID.isEmpty {
      opIDs.append(opID)
      if let eng = self.engine {
        let seeded = parseOperationTargets(eng: eng, opID: opID)
        for (devID, st) in seeded {
          if !order.contains(devID) { order.append(devID) }
          states[devID] = st
          alreadySent.insert(devID)
        }
      }
    }

    // Track in pendingFlushes so any trusted peers that connect a moment later also receive
    // this item and fold into the exact same aggregated send pill.
    pendingFlushes.removeAll { $0.historyID == historyID }
    pendingFlushes.append(PendingFlush(historyID: historyID, sentDeviceIDs: alreadySent))
    if pendingFlushes.count > Self.maxPendingFlushItems {
      pendingFlushes.removeFirst(pendingFlushes.count - Self.maxPendingFlushItems)
    }

    activeSendBatch = ActiveSendBatch(
      historyID: historyID,
      clipTitle: clipTitle,
      operationIDs: opIDs,
      deviceOrder: order,
      deviceStates: states,
      lastHighlightText: nil,
      graceExpired: false
    )
    scheduleSendOfflineGraceLocked(historyID: historyID)
    emitActiveSendBatchLocked()
  }

  private func scheduleSendOfflineGraceLocked(historyID: String) {
    sendGraceWorkItem?.cancel()
    let gen = activeGeneration
    let work = DispatchWorkItem { [weak self] in
      guard let self, self.activeGeneration == gen else { return }
      guard var batch = self.activeSendBatch, batch.historyID == historyID else { return }
      batch.graceExpired = true
      var changed = false
      for devID in batch.deviceOrder {
        if batch.deviceStates[devID] == .waiting {
          batch.deviceStates[devID] = .offline
          changed = true
        }
      }
      self.activeSendBatch = batch
      if changed || batch.deviceOrder.isEmpty {
        self.emitActiveSendBatchLocked()
      }
    }
    sendGraceWorkItem = work
    queue.asyncAfter(deadline: .now() + Self.sendOfflineGraceSeconds, execute: work)
  }

  private func pollActiveSendOperationsLocked() {
    guard let eng = self.engine, var batch = self.activeSendBatch, !batch.operationIDs.isEmpty else { return }
    var changed = false
    for opID in batch.operationIDs {
      let targets = parseOperationTargets(eng: eng, opID: opID)
      for (devID, newState) in targets {
        if !batch.deviceOrder.contains(devID) {
          batch.deviceOrder.append(devID)
          changed = true
        }
        let prev = batch.deviceStates[devID]
        // Do not let an older operation overwrite a newer retry or already-delivered state.
        if prev == .delivered && newState != .delivered {
          continue
        }
        if prev != newState {
          batch.deviceStates[devID] = newState
          changed = true
          if newState == .delivered {
            let name = deviceNames[devID] ?? String(devID.prefix(8))
            batch.lastHighlightText = "\(name) ✓"
          }
        }
      }
    }
    if changed {
      self.activeSendBatch = batch
      self.emitActiveSendBatchLocked()
    }
  }

  private func parseOperationTargets(eng: MobileEngine, opID: String) -> [(String, KeyboardSendDeviceState)] {
    var err: NSError?
    let raw = eng.getOperation(opID, error: &err)
    guard err == nil,
          let data = raw.data(using: .utf8),
          let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let targets = obj["targets"] as? [[String: Any]] else { return [] }

    var result: [(String, KeyboardSendDeviceState)] = []
    for t in targets {
      let devID = (t["target_id"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        ?? (t["peer_id"] as? String)
      guard let devID, !devID.isEmpty else { continue }
      let rawState = (t["state"] as? String) ?? ""
      switch rawState {
      case "queued":
        result.append((devID, .sending(progressPercent: nil)))
      case "sending":
        var pct: Int? = nil
        if let prog = t["progress"] as? [String: Any],
           let sent = (prog["bytes_sent"] as? NSNumber)?.int64Value,
           let total = (prog["total_bytes"] as? NSNumber)?.int64Value,
           total > 0 {
          pct = max(1, min(99, Int((Double(sent) / Double(total)) * 100.0)))
        }
        result.append((devID, .sending(progressPercent: pct)))
      case "sent", "discarded":
        result.append((devID, .delivered))
      case "failed":
        result.append((devID, .failed))
      default:
        break
      }
    }
    return result
  }

  private func emitActiveSendBatchLocked() {
    guard let batch = self.activeSendBatch else {
      emitSendBatch(nil)
      return
    }
    let devices: [KeyboardSendDeviceStatus] = batch.deviceOrder.map { devID in
      let name = deviceNames[devID] ?? String(devID.prefix(8))
      let state = batch.deviceStates[devID] ?? (batch.graceExpired ? .offline : .waiting)
      return KeyboardSendDeviceStatus(id: devID, name: name, state: state)
    }
    let isConnectingOnly = devices.isEmpty && !batch.graceExpired
    let dto = KeyboardSendBatchState(
      historyID: batch.historyID,
      clipTitle: batch.clipTitle,
      devices: devices,
      lastHighlightText: batch.lastHighlightText,
      isConnectingOnly: isConnectingOnly
    )
    emitSendBatch(dto)
  }

  private func seedExistingReceipts(_ eng: MobileEngine) {
    var err: NSError?
    let raw = eng.listReceived(&err)
    guard err == nil,
          let data = raw.data(using: .utf8),
          let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return }
    for entry in list {
      if let rid = entry["receipt_id"] as? String {
        seenReceiptIDs.insert(rid)
      }
    }
  }

  private func refreshPeersAndFlush() {
    guard let eng = self.engine else { return }
    var err: NSError?
    let raw = eng.listDevices(&err)
    guard err == nil,
          let data = raw.data(using: .utf8),
          let devs = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return }

    var trustedIDs: [String] = []
    var connectedIDs: [String] = []
    for d in devs {
      guard let id = d["id"] as? String else { continue }
      let name = (d["name"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        ?? (d["reported_name"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        ?? String(id.prefix(8))
      deviceNames[id] = name
      if (d["trust"] as? String) == "trusted" {
        trustedIDs.append(id)
        if let bindings = d["bindings"] as? [[String: Any]],
           bindings.contains(where: { ($0["connection"] as? String) == "connected" }) {
          connectedIDs.append(id)
        }
      }
    }
    trustedDeviceOrder = trustedIDs

    // Ensure active batch reflects newly discovered trusted devices.
    if var batch = self.activeSendBatch {
      var batchChanged = false
      for id in trustedIDs where !batch.deviceOrder.contains(id) {
        batch.deviceOrder.append(id)
        batch.deviceStates[id] = batch.graceExpired ? .offline : .waiting
        batchChanged = true
      }
      if batchChanged {
        self.activeSendBatch = batch
        self.emitActiveSendBatchLocked()
      }
    }

    if connectedIDs.isEmpty {
      emitState(.connecting)
      return
    }
    emitState(.connected(peerCount: connectedIDs.count))

    guard !pendingFlushes.isEmpty else { return }
    let connectedSet = Set(connectedIDs)
    var updatedActiveBatch = false
    for idx in pendingFlushes.indices {
      let unsent = Array(connectedSet.subtracting(pendingFlushes[idx].sentDeviceIDs))
      guard !unsent.isEmpty else { continue }
      let historyID = pendingFlushes[idx].historyID
      let req: [String: Any] = [
        "request_id": "ios-kb-flush-\(UUID().uuidString)",
        "history_id": historyID,
        "device_ids": unsent,
      ]
      guard let reqData = try? JSONSerialization.data(withJSONObject: req),
            let reqJSON = String(data: reqData, encoding: .utf8) else { continue }
      var sendErr: NSError?
      let resJSON = eng.resendHistoryItem(nil, reqJSON: reqJSON, error: &sendErr)
      if sendErr == nil {
        pendingFlushes[idx].sentDeviceIDs.formUnion(unsent)
        if var batch = self.activeSendBatch, batch.historyID == historyID {
          var opID = ""
          if let data = resJSON.data(using: .utf8),
             let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            opID = (obj["operation_id"] as? String) ?? ""
          }
          if !opID.isEmpty && !batch.operationIDs.contains(opID) {
            batch.operationIDs.append(opID)
          }
          for devID in unsent {
            if !batch.deviceOrder.contains(devID) { batch.deviceOrder.append(devID) }
            if batch.deviceStates[devID] != .delivered {
              batch.deviceStates[devID] = .sending(progressPercent: nil)
            }
          }
          self.activeSendBatch = batch
          updatedActiveBatch = true
        }
      }
    }
    if updatedActiveBatch {
      pollActiveSendOperationsLocked()
      emitActiveSendBatchLocked()
    }
  }

  private func handleIncomingReceivedLocked() {
    guard let eng = self.engine, let cacheDir = store.sharedEngineCacheDir else { return }
    var err: NSError?
    let raw = eng.listReceived(&err)
    guard err == nil,
          let data = raw.data(using: .utf8),
          let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return }

    // Process oldest-first so if multiple arrive at once, newest wins the system clipboard.
    for entry in list.reversed() {
      guard let rid = entry["receipt_id"] as? String, !seenReceiptIDs.contains(rid) else { continue }
      seenReceiptIDs.insert(rid)

      guard let content = entry["content"] as? [String: Any],
            let parts = content["parts"] as? [[String: Any]] else { continue }

      let senderName: String = {
        if let devID = (entry["peer_id"] as? String) ?? (entry["device_id"] as? String), !devID.isEmpty {
          return deviceNames[devID] ?? String(devID.prefix(8))
        }
        return strings.remoteDeviceFallback
      }()

      let beginReq = "{\"receipt_id\":\"\(rid)\",\"expected_epoch\":\"\",\"expected_sequence\":\"0\"}"
      var bErr: NSError?
      let ticketJSON = eng.beginApply(beginReq, error: &bErr)
      guard bErr == nil,
            let td = ticketJSON.data(using: .utf8),
            let ticket = try? JSONSerialization.jsonObject(with: td) as? [String: Any],
            let token = ticket["token"] as? String,
            let lid = ticket["lease_id"] as? String else { continue }
      defer { try? eng.releaseContent(lid) }

      var applied = false
      var appliedImage = false

      let imgPart = parts.first(where: {
        ($0["kind"] as? String) == "representation" && (($0["mime"] as? String)?.hasPrefix("image/") == true)
      }) ?? (parts.count == 1 ? parts.first(where: {
        ($0["kind"] as? String) == "file" && (($0["mime"] as? String)?.hasPrefix("image/") == true)
      }) : nil)

      if let imgPart,
         let partID = imgPart["id"] as? String {
        let declaredSize = (imgPart["size"] as? NSNumber)?.int64Value ?? 0
        if declaredSize <= Int64(Self.maxReceiveImageBytes) {
          let dest = cacheDir.appendingPathComponent("kb-recv-\(UUID().uuidString).img")
          var written: Int64 = 0
          try? eng.exportPart(toFile: lid, partID: partID, destPath: dest.path, ret0_: &written)
          defer { try? FileManager.default.removeItem(at: dest) }
          if let imgData = try? Data(contentsOf: dest),
             imgData.count <= Self.maxReceiveImageBytes,
             let image = UIImage(data: imgData) {
            let afterCC: Int = DispatchQueue.main.sync {
              UIPasteboard.general.image = image
              return UIPasteboard.general.changeCount
            }
            let hash = KeyboardSnapshotStore.sha256Hex(of: imgData)
            lastCheckedChangeCount = afterCC
            store.recordPasteboardState(changeCount: afterCC, contentSHA256: hash, origin: "applied")
            applied = true
            appliedImage = true
          }
        }
      } else if let txtPart = parts.first(where: {
        ($0["kind"] as? String) == "representation" && (($0["mime"] as? String) == "text/plain" || ($0["mime"] as? String) == "text/html")
      }),
                let partID = txtPart["id"] as? String {
        var rErr: NSError?
        let textJSON = eng.readLeasedText(lid, partID: partID, maxBytes: Self.maxTextBytes, error: &rErr)
        if rErr == nil,
           let pd = textJSON.data(using: .utf8),
           let po = try? JSONSerialization.jsonObject(with: pd) as? [String: Any],
           let text = po["text"] as? String,
           !text.isEmpty {
          let afterCC: Int = DispatchQueue.main.sync {
            UIPasteboard.general.string = text
            return UIPasteboard.general.changeCount
          }
          let hash = KeyboardSnapshotStore.sha256Hex(ofText: text)
          lastCheckedChangeCount = afterCC
          store.recordPasteboardState(changeCount: afterCC, contentSHA256: hash, origin: "applied")
          applied = true
        }
      }

      let outcome = applied ? "applied" : "failed"
      let reportJSON = "{\"token\":\"\(token)\",\"outcome\":\"\(outcome)\",\"epoch\":\"\",\"sequence\":\"0\"}"
      try? eng.reportApplyResult(reportJSON)

      if applied {
        emitToast(strings.toastReceivedAndCopied(from: senderName, isImage: appliedImage))
      }
    }
  }

  // MARK: - Live History Loading & Snapshot Mirroring

  private func scheduleHistoryRefresh(delay: TimeInterval) {
    refreshWorkItem?.cancel()
    let gen = activeGeneration
    let work = DispatchWorkItem { [weak self] in
      guard let self, self.activeGeneration == gen, let eng = self.engine else { return }
      self.refreshHistoryLocked(eng, writeSnapshot: true)
    }
    refreshWorkItem = work
    queue.asyncAfter(deadline: .now() + delay, execute: work)
  }

  private func refreshHistoryLocked(_ eng: MobileEngine, writeSnapshot: Bool) {
    store.ensureMediaDirs()
    guard let thumbsDir = store.thumbnailsDir, let imgsDir = store.imagesDir else { return }

    // Refresh device name map from Engine state.
    var stateErr: NSError?
    let stateJSON = eng.getState(&stateErr)
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

    let recentReq = "{\"limit\":\(Self.maxItems),\"filter\":{\"kinds\":[\"text\",\"image\"]}}"
    var listErr: NSError?
    let recentJSON = eng.listHistory(recentReq, error: &listErr)
    guard listErr == nil,
          let recentData = recentJSON.data(using: .utf8),
          let recentObj = try? JSONSerialization.jsonObject(with: recentData) as? [String: Any] else { return }

    let historyRevision = (recentObj["history_revision"] as? String) ?? "0"
    let recentItems = (recentObj["items"] as? [[String: Any]]) ?? []

    let pinnedReq = "{\"limit\":25,\"filter\":{\"kinds\":[\"text\",\"image\"],\"tags\":[\"pinned\"]}}"
    var pinErr: NSError?
    let pinnedJSON = eng.listHistory(pinnedReq, error: &pinErr)
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

    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
    var items: [KeyboardClipItem] = []
    var keepThumbs = Set<String>()
    var keepImages = Set<String>()
    var imageCount = 0
    let fm = FileManager.default

    for raw in merged {
      guard items.count < Self.maxItems,
            let id = raw["id"] as? String,
            let kind = raw["kind"] as? String else { continue }

      if let expiresAt = (raw["expires_at_ms"] as? NSNumber)?.int64Value, expiresAt > 0, expiresAt < nowMs {
        continue
      }

      let revision = String(describing: raw["revision"] ?? "1")
      let createdAtMs = (raw["created_at_ms"] as? NSNumber)?.int64Value ?? nowMs
      let tags = (raw["tags"] as? [String]) ?? []
      let pinned = tags.contains("pinned")
      let deviceName = resolveDeviceName(raw: raw)

      if kind == "image" {
        guard imageCount < Self.maxImages else { continue }
        var expErr: NSError?
        let expJSON = eng.exportHistoryImagePreview(id, error: &expErr)
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
        guard fm.fileExists(atPath: thumbDest.path) else { continue }
        keepThumbs.insert(thumbName)

        let imgName = "\(safeID).\(ext)"
        let imgDest = imgsDir.appendingPathComponent(imgName)
        var copiedImageName: String? = nil
        if let sizeVal = (try? srcURL.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
           Int64(sizeVal) <= Int64(Self.maxCaptureImageBytes) {
          if !fm.fileExists(atPath: imgDest.path) {
            try? fm.copyItem(at: srcURL, to: imgDest)
            try? fm.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: imgDest.path)
          }
          if fm.fileExists(atPath: imgDest.path) {
            copiedImageName = imgName
            keepImages.insert(imgName)
          }
        }

        let formatLabel = ext.uppercased()
        let metaText = dims.map { "\($0.width) × \($0.height) · \(formatLabel)" } ?? formatLabel
        items.append(KeyboardClipItem(
          id: id,
          revision: revision,
          kind: .image,
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

      var textErr: NSError?
      let prevJSON = eng.readHistoryTextPreview(id, maxBytes: Self.maxInsertTextBytes, error: &textErr)
      guard textErr == nil,
            let prevData = prevJSON.data(using: .utf8),
            let prevObj = try? JSONSerialization.jsonObject(with: prevData) as? [String: Any],
            let rawText = prevObj["text"] as? String else { continue }

      let trimmed = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !trimmed.isEmpty, !trimmed.hasPrefix("unic-pair:") else { continue }

      let classified = classifyText(rawText: rawText, trimmed: trimmed)
      items.append(KeyboardClipItem(
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

    lastHistoryRevision = historyRevision
    lastItems = items
    lastKeepThumbs = keepThumbs
    lastKeepImages = keepImages

    if writeSnapshot {
      store.saveSnapshot(
        items: items,
        historyRevision: historyRevision,
        selfDeviceName: selfDeviceName,
        keepThumbs: keepThumbs,
        keepImages: keepImages
      )
    }

    let captured = items
    Task { @MainActor [weak self] in
      self?.onItemsUpdated?(captured)
    }
  }

  // MARK: - Classification & Media Helpers

  private struct ClassifiedText {
    let kind: KeyboardItemKind
    let title: String
    let insertText: String
    let subtitle: String?
    let domain: String?
  }

  private func classifyText(rawText: String, trimmed: String) -> ClassifiedText {
    if let otp = extractOTP(from: trimmed) {
      let hint = (trimmed == otp.code || trimmed.replacingOccurrences(of: " ", with: "") == otp.code)
        ? "验证码 · 点按填入"
        : otp.hint
      return ClassifiedText(kind: .otp, title: otp.code, insertText: otp.code, subtitle: hint, domain: nil)
    }
    if !trimmed.contains("\n"),
       (trimmed.hasPrefix("http://") || trimmed.hasPrefix("https://")),
       let url = URL(string: trimmed),
       let host = url.host, !host.isEmpty {
      let cleanDomain = host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
      let pathSummary = url.path.isEmpty || url.path == "/" ? cleanDomain : "\(cleanDomain)\(url.path)"
      return ClassifiedText(
        kind: .url,
        title: String(pathSummary.prefix(Self.maxTitleChars)),
        insertText: trimmed,
        subtitle: trimmed,
        domain: cleanDomain
      )
    }
    let compactTitle = trimmed.replacingOccurrences(of: "\r\n", with: "\n").prefix(Self.maxTitleChars)
    return ClassifiedText(kind: .text, title: String(compactTitle), insertText: rawText, subtitle: nil, domain: nil)
  }

  private func extractOTP(from trimmed: String) -> (code: String, hint: String)? {
    guard trimmed.count <= 240 else { return nil }
    let stripped = trimmed.replacingOccurrences(of: " ", with: "").replacingOccurrences(of: "-", with: "")
    if stripped.count == 6, stripped.allSatisfy({ $0.isASCII && $0.isNumber }), trimmed.count <= 7 {
      return (stripped, "6 位验证码")
    }
    let lower = trimmed.lowercased()
    let keywords = [
      "验证码", "认证码", "校验码", "动态码", "安全码", "取件码", "确认码",
      "verification code", "security code", "passcode", "one-time",
      "otp", "2fa", "auth code", "login code", "sign-in code", "signin code"
    ]
    guard keywords.contains(where: { lower.contains($0) }) else { return nil }
    let pattern = "(?<![0-9A-Za-z])([0-9]{4,8})(?![0-9A-Za-z])"
    guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }
    let matches = regex.matches(in: trimmed, range: NSRange(trimmed.startIndex..., in: trimmed))
    var chosen: String?
    for match in matches {
      guard let r = Range(match.range(at: 1), in: trimmed) else { continue }
      let candidate = String(trimmed[r])
      if candidate.count == 6 { chosen = candidate; break }
      if chosen == nil { chosen = candidate }
    }
    guard let code = chosen else { return nil }
    var senderPrefix = ""
    if let open = trimmed.firstIndex(of: "【"), let close = trimmed.firstIndex(of: "】"), open < close {
      let inner = trimmed[trimmed.index(after: open)..<close]
      if !inner.isEmpty && inner.count <= 24 { senderPrefix = "\(inner) " }
    }
    return (code, "\(senderPrefix)验证码 · 自动提取")
  }

  private func resolveDeviceName(raw: [String: Any]) -> String {
    if let origin = raw["origin"] as? [String: Any],
       let originKind = origin["kind"] as? String {
      if originKind == "remote_device", let devID = origin["device_id"] as? String {
        return deviceNames[devID] ?? String(devID.prefix(8))
      }
      if originKind == "external" { return "外部渠道" }
      if originKind == "migration" { return "v1 迁移" }
    }
    if let srcApp = raw["source_application"] as? [String: Any],
       let appName = srcApp["name"] as? String, !appName.isEmpty {
      return appName
    }
    return selfDeviceName
  }

  private func safeFileStem(_ id: String) -> String {
    let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
    return String(id.unicodeScalars.map { allowed.contains($0) ? Character($0) : "_" }.prefix(80))
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
    if !FileManager.default.fileExists(atPath: destURL.path) {
      let options: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceThumbnailMaxPixelSize: maxPixel,
        kCGImageSourceShouldCacheImmediately: true,
      ]
      if let cgThumb = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) {
        let uiImg = UIImage(cgImage: cgThumb)
        if let jpeg = uiImg.jpegData(compressionQuality: 0.78) {
          try? store.writeAtomic(data: jpeg, to: destURL)
        }
      }
    }
    return dims
  }

  // MARK: - MainActor Emitters

  private func emitState(_ state: ConnectionState) {
    Task { @MainActor [weak self] in
      self?.onStateChanged?(state)
    }
  }

  private func emitToast(_ text: String) {
    Task { @MainActor [weak self] in
      self?.onToast?(text)
    }
  }

  private func emitSendBatch(_ batch: KeyboardSendBatchState?) {
    Task { @MainActor [weak self] in
      self?.onSendBatchUpdated?(batch)
    }
  }
}
