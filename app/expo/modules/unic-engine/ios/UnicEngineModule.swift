import AVFoundation
import Foundation
import Photos
import PhotosUI
@preconcurrency import ExpoModulesCore
import Network
import Security
import UIKit
import UnicCore
import UniformTypeIdentifiers

private final class UnicModuleException: GenericException<String> {
  override var reason: String { param }
}

private func formatGoError(_ error: Error) -> String {
  let raw = error.localizedDescription
  if let data = raw.data(using: .utf8),
     let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
     let code = obj["code"] as? String {
    return "[unic:\(code)] \(raw)"
  }
  let fallback = ["code": "internal", "message": "operation failed"]
  let fallbackData = (try? JSONSerialization.data(withJSONObject: fallback)) ?? Data()
  let fallbackJSON = String(data: fallbackData, encoding: .utf8) ?? "{\"code\":\"internal\",\"message\":\"operation failed\"}"
  return "[unic:internal] \(fallbackJSON)"
}

private func callGoString(_ body: (NSErrorPointer) -> String) throws -> String {
  var err: NSError?
  let result = body(&err)
  if let err = err {
    throw UnicModuleException(formatGoError(err))
  }
  return result
}

private final class EngineEventBridge: NSObject, MobileEventSinkProtocol, @unchecked Sendable {
  private let onEventCallback: (String) -> Void
  private let onResyncCallback: (String) -> Void

  init(onEvent: @escaping (String) -> Void, onResync: @escaping (String) -> Void) {
    self.onEventCallback = onEvent
    self.onResyncCallback = onResync
  }

  func onEvent(_ eventJSON: String?) {
    if let json = eventJSON {
      onEventCallback(json)
    }
  }

  func onResyncRequired(_ reason: String?) {
    onResyncCallback(reason ?? "overflow")
  }
}

private final class MigrationProgressBridge: NSObject, MobileMigrationProgressSinkProtocol, @unchecked Sendable {
  private let callback: (String, Int, Int) -> Void

  init(_ callback: @escaping (String, Int, Int) -> Void) {
    self.callback = callback
  }

  func onProgress(_ stage: String?, processed: Int, total: Int) {
    callback(stage ?? "import", processed, total)
  }
}

private final class KeychainSecretBridge: NSObject, MobileSecretProviderProtocol, @unchecked Sendable {
  func readSecret(_ service: String?, account: String?) throws -> Data {
    guard let service = service, let account = account else {
      throw NSError(domain: "app.uniclipboard.engine", code: -1, userInfo: [NSLocalizedDescriptionKey: "missing credential identifier"])
    }
    let query: [CFString: Any] = [
      kSecClass: kSecClassGenericPassword,
      kSecAttrService: service,
      kSecAttrAccount: account,
      kSecReturnData: true,
      kSecMatchLimit: kSecMatchLimitOne,
    ]
    var result: AnyObject?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    if status == errSecSuccess, let data = result as? Data, !data.isEmpty {
      return data
    }
    throw NSError(domain: "app.uniclipboard.engine", code: Int(status), userInfo: [NSLocalizedDescriptionKey: "keychain credential unavailable"])
  }
}

private final class SecurityScopedFileStream: NSObject, MobileFileStreamProtocol, @unchecked Sendable {
  private let url: URL
  private let handle: FileHandle
  private let hasSecurityScope: Bool
  private var closed = false
  private let lock = NSLock()

  init(url: URL) throws {
    self.url = url
    self.hasSecurityScope = url.startAccessingSecurityScopedResource()
    do {
      self.handle = try FileHandle(forReadingFrom: url)
    } catch {
      if self.hasSecurityScope {
        url.stopAccessingSecurityScopedResource()
      }
      throw error
    }
  }

  func readChunk(_ maxBytes: Int) throws -> Data {
    lock.lock()
    defer { lock.unlock() }
    if closed || maxBytes <= 0 {
      return Data()
    }
    return try handle.read(upToCount: maxBytes) ?? Data()
  }

  func close() throws {
    lock.lock()
    defer { lock.unlock() }
    if closed {
      return
    }
    closed = true
    try? handle.close()
    if hasSecurityScope {
      url.stopAccessingSecurityScopedResource()
    }
  }
}

private final class SecurityScopedFileOpener: NSObject, MobileFileOpenerProtocol, @unchecked Sendable {
  private let url: URL
  private let fileSize: Int64
  private let fileVersion: String

  init(url: URL, fileSize: Int64, version: String) {
    self.url = url
    self.fileSize = fileSize
    self.fileVersion = version
  }

  func open() throws -> MobileFileStreamProtocol {
    return try SecurityScopedFileStream(url: url)
  }

  func size() -> Int64 {
    return fileSize
  }

  func version() -> String {
    return fileVersion
  }
}

private final class DocumentPickerCoordinator: NSObject, UIDocumentPickerDelegate, @unchecked Sendable {
  private let completion: ([URL]?) -> Void

  init(completion: @escaping ([URL]?) -> Void) {
    self.completion = completion
  }

  func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    completion(urls)
  }

  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    completion(nil)
  }
}

private final class MediaPickerCoordinator: NSObject, UIImagePickerControllerDelegate,
  UINavigationControllerDelegate, PHPickerViewControllerDelegate {
  private let completion: (UIImage?) -> Void

  init(completion: @escaping (UIImage?) -> Void) {
    self.completion = completion
  }

  func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
    picker.dismiss(animated: true) { self.completion(nil) }
  }

  func imagePickerController(
    _ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
  ) {
    let image = info[.originalImage] as? UIImage
    picker.dismiss(animated: true) { self.completion(image) }
  }

  func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
    picker.dismiss(animated: true) {
      guard let provider = results.first?.itemProvider else {
        self.completion(nil)
        return
      }
      provider.loadObject(ofClass: UIImage.self) { image, _ in
        self.completion(image as? UIImage)
      }
    }
  }
}

public final class UnicEngineModule: Module {
  private let engineQueue = DispatchQueue(label: "app.uniclipboard.engine", qos: .userInitiated)
  private let keyboardBridge = KeyboardBridge()
  private var keyboardSyncWorkItem: DispatchWorkItem?
  private var engine: MobileEngine?
  private var setupSession: MobileMigrationSession?
  private var activeMigrationCancel: MobileCancelToken?
  private var pathMonitor: NWPathMonitor?
  private var activeMediaPicker: MediaPickerCoordinator?
  private var activeQRScanner: PairingQRScanner?
  private var automaticCapture = true
  private var activePickerCoordinator: DocumentPickerCoordinator?
  private var securityScopedSetupURL: URL?

  private var observationEpoch: String = ""
  private var observationSequence: UInt64 = 0
  private var lastProbedChangeCount: Int = -1
  private var lastAppliedChangeCount: Int = -1
  private var isForeground: Bool = true
  private var pasteboardObserver: NSObjectProtocol?

  private func updateHostCapabilities(_ eng: MobileEngine, foreground: Bool) {
    let caps: [String: Any] = [
      "read_formats": ["text/plain", "text/html", "image/png"],
      "write_formats": ["text/plain", "text/html", "image/png"],
      "read_files": true,
      "write_files": true,
      "observe": foreground ? "permission_required" : "unavailable",
      "apply": foreground ? "available" : "unavailable",
    ]
    if let data = try? JSONSerialization.data(withJSONObject: caps),
       let json = String(data: data, encoding: .utf8) {
      try? eng.updateHostCapabilities(json)
    }
  }

  private func ensureEmptyObservationBaseline(_ eng: MobileEngine) throws {
    if !self.observationEpoch.isEmpty {
      return
    }
    let newEpoch = "ios-\(UUID().uuidString.prefix(12))"
    let req: [String: Any] = [
      "epoch": newEpoch,
      "sequence": "1",
      "mode": "baseline",
    ]
    let data = try JSONSerialization.data(withJSONObject: req)
    let json = String(data: data, encoding: .utf8) ?? "{}"
    _ = try callGoString { eng.observeClipboard(json, error: $0) }
    self.observationEpoch = newEpoch
    self.observationSequence = 1
  }

  private func observeCurrentPasteboardChange(_ eng: MobileEngine) throws -> String {
    let pbResult: (Int, String?, UIImage?) = DispatchQueue.main.sync {
      let pb = UIPasteboard.general
      return (pb.changeCount, pb.string, pb.image)
    }
    let changeCount = pbResult.0
    self.lastProbedChangeCount = changeCount
    if changeCount == self.lastAppliedChangeCount && self.lastAppliedChangeCount >= 0 {
      return "{\"skipped\":true,\"reason\":\"self_applied\"}"
    }
    var leaseID: String?
    if let text = pbResult.1, !text.isEmpty {
      if text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("unic-pair:") {
        return "{\"skipped\":true,\"reason\":\"empty_or_unsupported\"}"
      }
      let hJSON = try callGoString { eng.importText(text, html: "", obsKey: "ios-pb:\(changeCount)", error: $0) }
      if let d = hJSON.data(using: .utf8), let obj = try? JSONSerialization.jsonObject(with: d) as? [String: Any] {
        leaseID = obj["lease_id"] as? String
      }
    } else if let img = pbResult.2, let png = img.pngData() {
      let hJSON = try callGoString { eng.importBytes("image/png", fileName: "", data: png, error: $0) }
      if let d = hJSON.data(using: .utf8), let obj = try? JSONSerialization.jsonObject(with: d) as? [String: Any] {
        leaseID = obj["lease_id"] as? String
      }
    } else {
      return "{\"skipped\":true,\"reason\":\"empty_or_unsupported\"}"
    }
    guard let lid = leaseID else {
      throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"failed to stage clipboard lease\"}")
    }
    defer { try? eng.releaseContent(lid) }
    try self.ensureEmptyObservationBaseline(eng)
    let nextSeq = self.observationSequence + 1
    let obsReq: [String: Any] = [
      "epoch": self.observationEpoch,
      "sequence": String(nextSeq),
      "mode": "change",
      "lease_id": lid,
    ]
    let obsData = try JSONSerialization.data(withJSONObject: obsReq)
    let obsJSON = String(data: obsData, encoding: .utf8) ?? "{}"
    let res = try callGoString { eng.observeClipboard(obsJSON, error: $0) }
    self.observationSequence = nextSeq
    return res
  }

  private func startPasteboardObserver() {
    guard pasteboardObserver == nil else { return }
    pasteboardObserver = NotificationCenter.default.addObserver(
      forName: UIPasteboard.changedNotification,
      object: nil,
      queue: nil
    ) { [weak self] _ in
      guard let self = self else { return }
      self.engineQueue.async {
        guard self.isForeground, self.automaticCapture, self.engine != nil else { return }
        let probe: (Int, Bool, Bool) = DispatchQueue.main.sync {
          let pb = UIPasteboard.general
          return (pb.changeCount, pb.hasStrings, pb.hasImages)
        }
        let changeCount = probe.0
        if changeCount == self.lastAppliedChangeCount || changeCount == self.lastProbedChangeCount {
          return
        }
        self.lastProbedChangeCount = changeCount
        guard probe.1 || probe.2 else { return }
        self.sendEvent("onClipboardChanged", [
          "changeCount": changeCount,
          "hasStrings": probe.1,
          "hasImages": probe.2,
        ])
      }
    }
  }

  private func stopPasteboardObserver() {
    if let obs = pasteboardObserver {
      NotificationCenter.default.removeObserver(obs)
      pasteboardObserver = nil
    }
  }

  private func reportApplyFailure(_ eng: MobileEngine, token: String, errorCode: String) {
    let payload: [String: Any] = [
      "token": token,
      "outcome": "failed",
      "error_code": errorCode,
    ]
    if let data = try? JSONSerialization.data(withJSONObject: payload),
       let json = String(data: data, encoding: .utf8) {
      try? eng.reportApplyResult(json)
    }
  }

  private func launchOverride(_ key: String) -> String? {
    let args = ProcessInfo.processInfo.arguments
    for i in 0..<args.count {
      let arg = args[i]
      if arg == "-\(key)" || arg == "--\(key)", i + 1 < args.count {
        let val = args[i + 1].trimmingCharacters(in: .whitespacesAndNewlines)
        if !val.isEmpty {
          return val
        }
      }
      if arg.hasPrefix("--\(key)=") {
        let val = String(arg.dropFirst("--\(key)=".count)).trimmingCharacters(in: .whitespacesAndNewlines)
        if !val.isEmpty {
          return val
        }
      }
      if arg.hasPrefix("-\(key)=") {
        let val = String(arg.dropFirst("-\(key)=".count)).trimmingCharacters(in: .whitespacesAndNewlines)
        if !val.isEmpty {
          return val
        }
      }
    }
    if let env = ProcessInfo.processInfo.environment[key]?.trimmingCharacters(in: .whitespacesAndNewlines),
       !env.isEmpty {
      return env
    }
    return nil
  }

  private func resolveDirectory(overrideEnv: String, fallbackSubdir: String, searchPath: FileManager.SearchPathDirectory) throws -> String {
    let fm = FileManager.default
    let url: URL
    if let raw = launchOverride(overrideEnv) {
      url = URL(fileURLWithPath: raw, isDirectory: true)
    } else {
      guard let base = fm.urls(for: searchPath, in: .userDomainMask).first else {
        throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"unavailable app container\"}")
      }
      url = base.appendingPathComponent(fallbackSubdir, isDirectory: true)
    }
    try fm.createDirectory(at: url, withIntermediateDirectories: true)
    return url.resolvingSymlinksInPath().path
  }

  private func defaultV1SourceDirectory() -> String {
    if let raw = launchOverride("UNIC_MOBILE_V1_DIR") {
      return URL(fileURLWithPath: raw, isDirectory: true).resolvingSymlinksInPath().path
    }
    let fm = FileManager.default
    if let appSupport = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
      return appSupport.appendingPathComponent("UniClipboard", isDirectory: true).resolvingSymlinksInPath().path
    }
    return ""
  }

  private func legacyMobileConfigJSON() -> String {
    let fm = FileManager.default
    var databaseCandidates: [String] = []
    var attachmentRoots: [String] = []

    if let rawDB = launchOverride("UNIC_MOBILE_LEGACY_DB") {
      databaseCandidates.append(URL(fileURLWithPath: rawDB).resolvingSymlinksInPath().path)
    }
    if let rawAttachments = launchOverride("UNIC_MOBILE_LEGACY_ATTACHMENTS") {
      for segment in rawAttachments.split(separator: ":") {
        let trimmed = segment.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
          attachmentRoots.append(URL(fileURLWithPath: trimmed, isDirectory: true).resolvingSymlinksInPath().path)
        }
      }
    }

    var groupIDs: [String] = []
    if let bundleID = Bundle.main.bundleIdentifier, !bundleID.isEmpty {
      groupIDs.append("group.\(bundleID)")
      if bundleID == "app.uniclipboard.UniClipboard" {
        groupIDs.append("group.app.uniclipboard.ios")
      }
    }
    for groupID in groupIDs {
      if let container = fm.containerURL(forSecurityApplicationGroupIdentifier: groupID) {
        let dbURL = container
          .appendingPathComponent("Databases", isDirectory: true)
          .appendingPathComponent("uniclipboard.db")
        databaseCandidates.append(dbURL.resolvingSymlinksInPath().path)
        let payloadsURL = container.appendingPathComponent("payloads", isDirectory: true)
        attachmentRoots.append(payloadsURL.resolvingSymlinksInPath().path)
      }
    }

    if let docs = fm.urls(for: .documentDirectory, in: .userDomainMask).first {
      let sandboxDB = docs
        .appendingPathComponent("SQLite", isDirectory: true)
        .appendingPathComponent("uniclipboard.db")
      databaseCandidates.append(sandboxDB.resolvingSymlinksInPath().path)
      let clipboardsDir = docs.appendingPathComponent("clipboards", isDirectory: true)
      attachmentRoots.append(clipboardsDir.resolvingSymlinksInPath().path)
    }
    if let caches = fm.urls(for: .cachesDirectory, in: .userDomainMask).first {
      let tempFilesDir = caches.appendingPathComponent("temp_files", isDirectory: true)
      attachmentRoots.append(tempFilesDir.resolvingSymlinksInPath().path)
    }

    let payload: [String: Any] = [
      "database_candidates": databaseCandidates,
      "attachment_roots": attachmentRoots,
    ]
    guard let data = try? JSONSerialization.data(withJSONObject: payload),
          let text = String(data: data, encoding: .utf8) else {
      return "{}"
    }
    return text
  }

  private func dataDirectoryPath() throws -> String {
    return try resolveDirectory(overrideEnv: "UNIC_MOBILE_DATA_DIR", fallbackSubdir: "unic-v2", searchPath: .applicationSupportDirectory)
  }

  private func cacheDirectoryPath() throws -> String {
    return try resolveDirectory(overrideEnv: "UNIC_MOBILE_CACHE_DIR", fallbackSubdir: "unic-cache", searchPath: .cachesDirectory)
  }

  private func ensureSetupSession() throws -> MobileMigrationSession {
    if let existing = setupSession {
      return existing
    }
    let dataDir = try dataDirectoryPath()
    let defaultV1 = defaultV1SourceDirectory()
    var err: NSError?
    guard let created = MobileOpenSetupFlow(dataDir, defaultV1, &err) else {
      if let err = err {
        throw UnicModuleException(formatGoError(err))
      }
      throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"open setup flow failed\"}")
    }
    try created.configureLegacyMobile(legacyMobileConfigJSON())
    setupSession = created
    return created
  }

  private func requireEngine() throws -> MobileEngine {
    guard let eng = engine else {
      throw UnicModuleException("[unic:invalid_state] {\"code\":\"invalid_state\",\"message\":\"engine is not initialized\"}")
    }
    return eng
  }

  private func shutdownEngineInternal() {
    keyboardSyncWorkItem?.cancel()
    keyboardSyncWorkItem = nil
    if let eng = engine {
      try? eng.close(nil)
      engine = nil
    }
  }

  private func scheduleKeyboardSnapshotPublish(delay: TimeInterval = 0.35) {
    keyboardSyncWorkItem?.cancel()
    let work = DispatchWorkItem { [weak self] in
      guard let self, let eng = self.engine else { return }
      _ = try? self.keyboardBridge.publishSnapshot(engine: eng)
    }
    keyboardSyncWorkItem = work
    engineQueue.asyncAfter(deadline: .now() + delay, execute: work)
  }

  private func startNetworkMonitor() {
    guard pathMonitor == nil else { return }
    let monitor = NWPathMonitor()
    monitor.pathUpdateHandler = { [weak self] _ in
      guard let self = self else { return }
      self.engineQueue.async {
        try? self.engine?.notifyNetworkChanged()
      }
    }
    monitor.start(queue: DispatchQueue(label: "app.uniclipboard.netmon"))
    pathMonitor = monitor
  }

  private func stopNetworkMonitor() {
    pathMonitor?.cancel()
    pathMonitor = nil
  }

  private func stopSecurityScopedSetupURL() {
    if let prev = securityScopedSetupURL {
      prev.stopAccessingSecurityScopedResource()
      securityScopedSetupURL = nil
    }
  }

  private func setIdleTimerDisabled(_ disabled: Bool) {
    DispatchQueue.main.async {
      UIApplication.shared.isIdleTimerDisabled = disabled
    }
  }

  private func presenter() -> UIViewController? {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    let window = scenes.flatMap { $0.windows }.first(where: { $0.isKeyWindow })
    var controller = window?.rootViewController
    while let presented = controller?.presentedViewController {
      controller = presented
    }
    return controller
  }

  private func presentDocumentPicker(types: [UTType], completion: @escaping ([URL]?) -> Void) {
    DispatchQueue.main.async { [weak self] in
      guard let self, let presenter = self.presenter(), self.activePickerCoordinator == nil else {
        completion(nil)
        return
      }
      let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: false)
      picker.allowsMultipleSelection = false
      let coordinator = DocumentPickerCoordinator { [weak self] urls in
        self?.activePickerCoordinator = nil
        completion(urls)
      }
      self.activePickerCoordinator = coordinator
      picker.delegate = coordinator
      presenter.present(picker, animated: true)
    }
  }

  private func stageExport(_ id: String, imagesOnly: Bool) throws -> (URL, [URL]) {
    let eng = try requireEngine()
    let json = try callGoString { eng.stageHistoryExport(id, imagesOnly: imagesOnly, error: $0) }
    guard let value = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
          let dir = value["directory"] as? String,
          let files = value["files"] as? [[String: String]] else {
      throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"invalid export\"}")
    }
    return (URL(fileURLWithPath: dir), files.compactMap { $0["path"].map { URL(fileURLWithPath: $0) } })
  }

  private func commitPickedImage(_ image: UIImage, promise: Promise) {
    engineQueue.async {
      do {
        let eng = try self.requireEngine()
        let pixelWidth = image.size.width * image.scale
        let pixelHeight = image.size.height * image.scale
        let scale = min(1, 4096 / max(pixelWidth, pixelHeight))
        let size = CGSize(width: max(1, pixelWidth * scale), height: max(1, pixelHeight * scale))
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let data = UIGraphicsImageRenderer(size: size, format: format).pngData { _ in
          image.draw(in: CGRect(origin: .zero, size: size))
        }
        guard !data.isEmpty else {
          throw UnicModuleException("[unic:invalid_argument] {\"code\":\"invalid_argument\",\"message\":\"invalid image\"}")
        }
        let handle = try callGoString { eng.importBytes("image/png", fileName: "", data: data, error: $0) }
        guard let value = try JSONSerialization.jsonObject(with: Data(handle.utf8)) as? [String: Any],
              let lease = value["lease_id"] as? String else {
          throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"invalid content handle\"}")
        }
        defer { try? eng.releaseContent(lease) }
        let request = try JSONSerialization.data(withJSONObject: [
          "request_id": "mobile-image-\(UUID())",
          "lease_id": lease,
        ])
        let submission = try callGoString { eng.commitContent(nil, reqJSON: String(decoding: request, as: UTF8.self), error: $0) }
        promise.resolve("{\"cancelled\":false,\"submission\":\(submission)}")
      } catch let err as UnicModuleException {
        promise.reject(err)
      } catch {
        promise.reject(UnicModuleException(formatGoError(error)))
      }
    }
  }

  private func presentImagePicker(camera: Bool, promise: Promise) {
    guard let presenter = presenter(), activeMediaPicker == nil else {
      promise.reject(UnicModuleException("[unic:invalid_state] {\"code\":\"invalid_state\",\"message\":\"no available presenter\"}"))
      return
    }
    if camera && !UIImagePickerController.isSourceTypeAvailable(.camera) {
      promise.reject(UnicModuleException("[unic:unsupported] {\"code\":\"unsupported\",\"message\":\"camera unavailable\"}"))
      return
    }
    let coordinator = MediaPickerCoordinator { [weak self] image in
      DispatchQueue.main.async {
        self?.activeMediaPicker = nil
        if let self, let image {
          self.commitPickedImage(image, promise: promise)
        } else {
          promise.resolve("{\"cancelled\":true}")
        }
      }
    }
    activeMediaPicker = coordinator
    if camera {
      let picker = UIImagePickerController()
      picker.sourceType = .camera
      picker.delegate = coordinator
      presenter.present(picker, animated: true)
    } else {
      var config = PHPickerConfiguration(photoLibrary: .shared())
      config.filter = .images
      config.selectionLimit = 1
      let picker = PHPickerViewController(configuration: config)
      picker.delegate = coordinator
      presenter.present(picker, animated: true)
    }
  }

  public func definition() -> ModuleDefinition {
    Name("UnicEngine")

    Events("onEngineEvent", "onEngineResync", "onMigrationProgress", "onClipboardChanged", "onIncomingShare")

    OnAppBecomesActive { [weak self] in
      guard let self = self else { return }
      self.engineQueue.async {
        self.isForeground = true
        guard let eng = self.engine else { return }
        self.updateHostCapabilities(eng, foreground: true)
        try? eng.setRuntimeState("active")
        if self.keyboardBridge.drainPendingActions(engine: eng) > 0 {
          self.scheduleKeyboardSnapshotPublish(delay: 0.1)
        } else {
          self.scheduleKeyboardSnapshotPublish(delay: 0.4)
        }
        let probe: (Int, Bool, Bool) = DispatchQueue.main.sync {
          let pb = UIPasteboard.general
          return (pb.changeCount, pb.hasStrings, pb.hasImages)
        }
        let changeCount = probe.0
        if self.automaticCapture && changeCount != self.lastProbedChangeCount &&
           changeCount != self.lastAppliedChangeCount &&
           (probe.1 || probe.2) {
          self.lastProbedChangeCount = changeCount
          self.sendEvent("onClipboardChanged", [
            "changeCount": changeCount,
            "hasStrings": probe.1,
            "hasImages": probe.2,
          ])
        }
      }
    }

    OnAppEntersBackground { [weak self] in
      guard let self = self else { return }
      self.engineQueue.async {
        self.isForeground = false
        self.observationEpoch = ""
        self.observationSequence = 0
        guard let eng = self.engine else { return }
        self.keyboardSyncWorkItem?.cancel()
        self.keyboardSyncWorkItem = nil
        _ = try? self.keyboardBridge.publishSnapshot(engine: eng)
        self.updateHostCapabilities(eng, foreground: false)
        try? eng.setRuntimeState("suspended")
      }
    }

    OnDestroy { [weak self] in
      guard let self = self else { return }
      self.activeMigrationCancel?.cancel()
      self.stopPasteboardObserver()
      self.stopNetworkMonitor()
      self.stopSecurityScopedSetupURL()
      self.setIdleTimerDisabled(false)
      self.engineQueue.sync {
        self.shutdownEngineInternal()
      }
    }

    // ---- First-install Setup & v1 Migration ----

    AsyncFunction("openSetupFlow") { () -> String in
      let sess = try self.ensureSetupSession()
      return try callGoString { sess.getState($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("getSetupState") { () -> String in
      let sess = try self.ensureSetupSession()
      return try callGoString { sess.getState($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("detectSetupSource") { (dir: String) -> String in
      let sess = try self.ensureSetupSession()
      return try callGoString { sess.detectSource(dir, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("selectSetupCustomSource") { (promise: Promise) in
      self.presentDocumentPicker(types: [.folder]) { [weak self] urls in
        guard let self = self else {
          promise.resolve("{\"cancelled\":true}")
          return
        }
        guard let pickedURL = urls?.first else {
          promise.resolve("{\"cancelled\":true}")
          return
        }
        self.engineQueue.async {
          let didAccess = pickedURL.startAccessingSecurityScopedResource()
          do {
            let sess = try self.ensureSetupSession()
            let resolvedPath = pickedURL.resolvingSymlinksInPath().path
            let resultJSON = try callGoString { sess.selectSource(resolvedPath, error: $0) }
            self.stopSecurityScopedSetupURL()
            if didAccess {
              self.securityScopedSetupURL = pickedURL
            }
            var payload: [String: Any] = ["cancelled": false]
            if let data = resultJSON.data(using: .utf8),
               let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
              if let src = parsed["source"] as? String {
                payload["source"] = src
              }
              if let kind = parsed["source_kind"] as? String {
                payload["source_kind"] = kind
              }
              if let reqUnlock = parsed["requires_unlock"] as? Bool {
                payload["requires_unlock"] = reqUnlock
              }
            }
            let outData = try JSONSerialization.data(withJSONObject: payload)
            promise.resolve(String(data: outData, encoding: .utf8) ?? "{\"cancelled\":false}")
          } catch let err as UnicModuleException {
            if didAccess { pickedURL.stopAccessingSecurityScopedResource() }
            promise.reject(err)
          } catch {
            if didAccess { pickedURL.stopAccessingSecurityScopedResource() }
            promise.reject(UnicModuleException(formatGoError(error)))
          }
        }
      }
    }

    AsyncFunction("beginLaterMigration") { () in
      do {
        self.shutdownEngineInternal()
        let sess = try self.ensureSetupSession()
        try sess.beginLaterMigration()
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("runSetupMigration") { (passphrase: String?) -> String in
      self.shutdownEngineInternal()
      self.setIdleTimerDisabled(true)
      defer {
        self.setIdleTimerDisabled(false)
        self.activeMigrationCancel = nil
      }
      let sess = try self.ensureSetupSession()
      let token = MobileNewCancelToken()
      self.activeMigrationCancel = token
      let pwData: Data? = (passphrase?.isEmpty == false) ? passphrase!.data(using: .utf8) : nil
      let sink = MigrationProgressBridge { [weak self] stage, processed, total in
        self?.sendEvent("onMigrationProgress", [
          "stage": stage,
          "processed": processed,
          "total": total,
        ])
      }
      return try callGoString {
        sess.run(token, passphrase: pwData, secrets: KeychainSecretBridge(), sink: sink, error: $0)
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("cancelSetupMigration") { () in
      self.activeMigrationCancel?.cancel()
    }

    AsyncFunction("markSetupComplete") { () in
      do {
        let sess = try self.ensureSetupSession()
        try sess.markComplete()
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    // ---- Engine Lifecycle ----

    AsyncFunction("initializeEngine") { (deviceName: String?) -> String in
      if let existing = self.engine {
        return try callGoString { existing.getState($0) }
      }
      let dataDir = try self.dataDirectoryPath()
      let cacheDir = try self.cacheDirectoryPath()
      let resolvedName: String = DispatchQueue.main.sync {
        let candidate = deviceName?.trimmingCharacters(in: .whitespacesAndNewlines)
        return (candidate?.isEmpty == false) ? candidate! : UIDevice.current.name
      }
      let cfg: [String: Any] = [
        "data_dir": dataDir,
        "cache_dir": cacheDir,
        "device_name": resolvedName,
        "host_capabilities": [
          "read_formats": ["text/plain", "text/html", "image/png"],
          "write_formats": ["text/plain", "text/html", "image/png"],
          "read_files": true,
          "write_files": true,
          "observe": self.isForeground ? "permission_required" : "unavailable",
          "apply": self.isForeground ? "available" : "unavailable",
        ],
      ]
      let cfgData = try JSONSerialization.data(withJSONObject: cfg)
      let cfgJSON = String(data: cfgData, encoding: .utf8) ?? "{}"
      var newErr: NSError?
      guard let eng = MobileNewEngine(cfgJSON, &newErr) else {
        if let newErr = newErr {
          throw UnicModuleException(formatGoError(newErr))
        }
        throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"new engine failed\"}")
      }
      let sink = EngineEventBridge(
        onEvent: { [weak self] eventJSON in
          self?.sendEvent("onEngineEvent", ["event": eventJSON])
          if eventJSON.contains("history") || eventJSON.contains("received") || eventJSON.contains("device") {
            self?.engineQueue.async {
              self?.scheduleKeyboardSnapshotPublish(delay: 0.35)
            }
          }
        },
        onResync: { [weak self] reason in
          self?.sendEvent("onEngineResync", ["reason": reason])
          self?.engineQueue.async {
            self?.scheduleKeyboardSnapshotPublish(delay: 0.35)
          }
        }
      )
      do {
        let snapshotJSON = try callGoString { eng.subscribe(sink, error: $0) }
        try eng.setRuntimeState(self.isForeground ? "active" : "suspended")
        try eng.start(nil)
        self.engine = eng
        self.observationEpoch = ""
        self.observationSequence = 0
        self.lastProbedChangeCount = DispatchQueue.main.sync { UIPasteboard.general.changeCount }
        self.startNetworkMonitor()
        self.startPasteboardObserver()
        self.scheduleKeyboardSnapshotPublish(delay: 0.2)
        let latest = try callGoString { eng.getState($0) }
        return latest.isEmpty ? snapshotJSON : latest
      } catch let err as UnicModuleException {
        try? eng.close(nil)
        throw err
      } catch {
        try? eng.close(nil)
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("shutdownEngine") { () in
      self.stopPasteboardObserver()
      self.stopNetworkMonitor()
      self.observationEpoch = ""
      self.observationSequence = 0
      self.shutdownEngineInternal()
    }.runOnQueue(engineQueue)

    AsyncFunction("listIncomingShares") { () -> String in
      try IncomingShare().list()
    }.runOnQueue(engineQueue)

    AsyncFunction("saveIncomingShare") { (id: String) -> String in
      let inbox = IncomingShare()
      guard let item = inbox.item(id) else {
        throw UnicModuleException("[unic:not_found] {\"code\":\"not_found\",\"message\":\"shared item unavailable\"}")
      }
      let engine = try self.requireEngine()
      let result = try callGoString { engine.commitSharedItem(item.path, deviceIDsJSON: "", error: $0) }
      try inbox.remove(id)
      return result
    }.runOnQueue(engineQueue)

    // ---- iOS Keyboard Extension Bridge ----

    AsyncFunction("getKeyboardExtensionStatus") { () -> String in
      self.keyboardBridge.statusJSON()
    }.runOnQueue(engineQueue)

    AsyncFunction("setKeyboardPreferences") { (soundEnabled: Bool, hapticEnabled: Bool) -> String in
      do {
        try self.keyboardBridge.savePreferences(soundEnabled: soundEnabled, hapticEnabled: hapticEnabled)
        return self.keyboardBridge.statusJSON()
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("syncKeyboardSnapshot") { () -> String in
      let eng = try self.requireEngine()
      do {
        let count = try self.keyboardBridge.publishSnapshot(engine: eng)
        return "{\"count\":\(count)}"
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("getState") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getState($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("getTelemetryState") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getTelemetryState($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("setTelemetryEnabled") { (enabled: Bool) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.setTelemetryEnabled(enabled, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("exportRecentDiagnosticLogs") { () -> String in
      try self.requireEngine().exportRecentDiagnosticLogs()
    }.runOnQueue(engineQueue)

    AsyncFunction("getCapabilities") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getCapabilities($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("updateSettings") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.updateSettings(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    // ---- History & Content ----

    AsyncFunction("getFileCacheState") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getFileCacheState($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("setAutomaticCapture") { (enabled: Bool) in
      self.automaticCapture = enabled
    }.runOnQueue(engineQueue)

    AsyncFunction("clearHistoryPreviews") { () in
      try self.requireEngine().clearHistoryPreviews()
    }.runOnQueue(engineQueue)

    AsyncFunction("reconcileHistoryPreviews") { () in
      try self.requireEngine().reconcileHistoryPreviews()
    }.runOnQueue(engineQueue)

    AsyncFunction("historyFileMetadata") { (historyID: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.historyFileMetadata(historyID, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("listHistoryFolder") { (id: String, limit: Int) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listHistoryFolder(id, limit: limit, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("copyPairingTicket") { (text: String, expiresAtMs: Double) in
      _ = try self.requireEngine()
      let expires = Date(timeIntervalSince1970: expiresAtMs / 1000)
      guard expires > Date() else {
        throw UnicModuleException("[unic:expired] {\"code\":\"expired\",\"message\":\"pairing ticket expired\"}")
      }
      self.lastAppliedChangeCount = DispatchQueue.main.sync {
        let clipboard = UIPasteboard.general
        clipboard.setItems([[UTType.utf8PlainText.identifier: text]], options: [.localOnly: true, .expirationDate: expires])
        return clipboard.changeCount
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("copyPlainText") { (text: String) in
      _ = try self.requireEngine()
      self.lastAppliedChangeCount = DispatchQueue.main.sync {
        UIPasteboard.general.string = text
        return UIPasteboard.general.changeCount
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("pickAndCommitImage") { (camera: Bool, promise: Promise) in
      DispatchQueue.main.async {
        if camera && AVCaptureDevice.authorizationStatus(for: .video) != .authorized {
          AVCaptureDevice.requestAccess(for: .video) { granted in
            DispatchQueue.main.async {
              if granted {
                self.presentImagePicker(camera: true, promise: promise)
              } else {
                promise.reject(UnicModuleException("[unic:permission_denied] {\"code\":\"permission_denied\",\"message\":\"camera access denied\"}"))
              }
            }
          }
        } else {
          self.presentImagePicker(camera: camera, promise: promise)
        }
      }
    }

    AsyncFunction("scanQRCode") { (cancelLabel: String, hint: String, promise: Promise) in
      Task { @MainActor in
        guard self.activeQRScanner == nil else {
          promise.reject(UnicModuleException("[unic:invalid_state] {\"code\":\"invalid_state\",\"message\":\"QR scanner already active\"}"))
          return
        }
        let scanner = PairingQRScanner(
          cancelLabel: cancelLabel,
          hint: hint,
          promise: promise,
          onFinish: { [weak self] in
            self?.activeQRScanner = nil
          }
        )
        self.activeQRScanner = scanner
        scanner.start()
      }
    }

    AsyncFunction("exportHistoryMedia") { (id: String, action: String, promise: Promise) in
      self.engineQueue.async {
        do {
          let (dir, files) = try self.stageExport(id, imagesOnly: action == "photos")
          let cleanup = { try? FileManager.default.removeItem(at: dir) }
          if action == "photos" {
            PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
              guard status == .authorized || status == .limited else {
                cleanup()
                promise.reject(UnicModuleException("[unic:permission_denied] {\"code\":\"permission_denied\",\"message\":\"photo access denied\"}"))
                return
              }
              PHPhotoLibrary.shared().performChanges {
                for file in files {
                  let request = PHAssetCreationRequest.forAsset()
                  request.addResource(with: .photo, fileURL: file, options: nil)
                }
              } completionHandler: { success, error in
                cleanup()
                if success { promise.resolve(true) }
                else { promise.reject(UnicModuleException(error.map(formatGoError) ?? "[unic:internal] {}")) }
              }
            }
          } else {
            DispatchQueue.main.async {
              guard let presenter = self.presenter(), self.activePickerCoordinator == nil else {
                cleanup()
                promise.reject(UnicModuleException("[unic:invalid_state] {\"code\":\"invalid_state\",\"message\":\"no available presenter\"}"))
                return
              }
              let picker = UIDocumentPickerViewController(forExporting: files, asCopy: true)
              let coordinator = DocumentPickerCoordinator { [weak self] urls in
                self?.activePickerCoordinator = nil
                cleanup()
                promise.resolve(urls != nil)
              }
              self.activePickerCoordinator = coordinator
              picker.delegate = coordinator
              presenter.present(picker, animated: true)
            }
          }
        } catch let err as UnicModuleException {
          promise.reject(err)
        } catch {
          promise.reject(UnicModuleException(formatGoError(error)))
        }
      }
    }

    AsyncFunction("shareHistoryMedia") { (idsJSON: String, text: String?, promise: Promise) in
      self.engineQueue.async {
        var directories: [URL] = []
        do {
          let ids = try JSONDecoder().decode([String].self, from: Data(idsJSON.utf8))
          var files: [URL] = []
          for id in ids {
            let (dir, parts) = try self.stageExport(id, imagesOnly: false)
            directories.append(dir)
            files.append(contentsOf: parts)
          }
          DispatchQueue.main.async {
            let cleanup = { for dir in directories { try? FileManager.default.removeItem(at: dir) } }
            var items: [Any] = files
            if let text, !text.isEmpty { items.insert(text, at: 0) }
            guard let presenter = self.presenter(), !items.isEmpty else {
              cleanup()
              promise.reject(UnicModuleException("[unic:invalid_state] {\"code\":\"invalid_state\",\"message\":\"no available presenter\"}"))
              return
            }
            let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
            sheet.popoverPresentationController?.sourceView = presenter.view
            sheet.popoverPresentationController?.sourceRect = CGRect(
              x: presenter.view.bounds.midX, y: presenter.view.bounds.midY, width: 1, height: 1
            )
            sheet.completionWithItemsHandler = { _, _, _, error in
              cleanup()
              if let error { promise.reject(UnicModuleException(formatGoError(error))) }
              else { promise.resolve(nil) }
            }
            presenter.present(sheet, animated: true)
          }
        } catch {
          for dir in directories { try? FileManager.default.removeItem(at: dir) }
          promise.reject((error as? UnicModuleException) ?? UnicModuleException(formatGoError(error)))
        }
      }
    }

    AsyncFunction("listHistory") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listHistory(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("getHistoryItem") { (historyID: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getHistoryItem(historyID, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("readHistoryTextPreview") { (historyID: String, maxBytes: Int) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.readHistoryTextPreview(historyID, maxBytes: maxBytes, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("readHistoryFullText") { (historyID: String, maxBytes: Int) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.readHistoryFullText(historyID, maxBytes: maxBytes, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("exportHistoryImagePreview") { (historyID: String) -> String in
      let eng = try self.requireEngine()
      let json = try callGoString { eng.exportHistoryImagePreview(historyID, error: $0) }
      var payload = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any] ?? [:]
      if let path = payload.removeValue(forKey: "path") as? String {
        payload["uri"] = URL(fileURLWithPath: path).absoluteString
      }
      let out = try JSONSerialization.data(withJSONObject: payload)
      return String(data: out, encoding: .utf8) ?? "{}"
    }.runOnQueue(engineQueue)

    AsyncFunction("updateHistoryItem") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.updateHistoryItem(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("deleteHistoryItem") { (historyID: String, expectedRevision: String) in
      do {
        try self.requireEngine().deleteHistoryItem(historyID, expectedRevision: expectedRevision)
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("listHistoryTags") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listHistoryTags($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("searchHistory") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.searchHistory(nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("countHistorySearches") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.countHistorySearches(nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("commitText") { (text: String, deviceIdsJson: String?) -> String in
      let eng = try self.requireEngine()
      let handleJSON = try callGoString { eng.importText(text, html: "", obsKey: "", error: $0) }
      guard let hData = handleJSON.data(using: .utf8),
            let hObj = try JSONSerialization.jsonObject(with: hData) as? [String: Any],
            let leaseID = hObj["lease_id"] as? String else {
        throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"invalid content handle\"}")
      }
      defer { try? eng.releaseContent(leaseID) }
      var commitReq: [String: Any] = [
        "request_id": "mobile-text-\(UUID().uuidString)",
        "lease_id": leaseID,
      ]
      if let deviceIdsJson = deviceIdsJson,
         let dData = deviceIdsJson.data(using: .utf8),
         let ids = try? JSONSerialization.jsonObject(with: dData) as? [String],
         !ids.isEmpty {
        commitReq["device_ids"] = ids
      }
      let reqData = try JSONSerialization.data(withJSONObject: commitReq)
      let reqJSON = String(data: reqData, encoding: .utf8) ?? "{}"
      return try callGoString { eng.commitContent(nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("pickAndCommitFile") { (deviceIdsJson: String?, promise: Promise) in
      self.presentDocumentPicker(types: [.item]) { [weak self] urls in
        guard let self = self else {
          promise.resolve("{\"cancelled\":true}")
          return
        }
        guard let pickedURL = urls?.first else {
          promise.resolve("{\"cancelled\":true}")
          return
        }
        self.engineQueue.async {
          let didAccess = pickedURL.startAccessingSecurityScopedResource()
          defer {
            if didAccess { pickedURL.stopAccessingSecurityScopedResource() }
          }
          do {
            let eng = try self.requireEngine()
            let rv = try? pickedURL.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey, .contentTypeKey, .nameKey])
            let fileSize = Int64(rv?.fileSize ?? 0)
            let mtime = rv?.contentModificationDate?.timeIntervalSince1970 ?? 0
            let version = "mtime:\(Int64(mtime * 1000));size:\(fileSize)"
            let mime = rv?.contentType?.preferredMIMEType ?? "application/octet-stream"
            let displayName = rv?.name ?? pickedURL.lastPathComponent

            let opener = SecurityScopedFileOpener(url: pickedURL, fileSize: fileSize, version: version)
            let handleJSON = try callGoString {
              eng.importFileSource(displayName, mime: mime, copyIntoEngine: true, opener: opener, error: $0)
            }
            guard let hData = handleJSON.data(using: .utf8),
                  let hObj = try JSONSerialization.jsonObject(with: hData) as? [String: Any],
                  let leaseID = hObj["lease_id"] as? String else {
              throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"invalid file handle\"}")
            }
            defer { try? eng.releaseContent(leaseID) }

            var commitReq: [String: Any] = [
              "request_id": "mobile-file-\(UUID().uuidString)",
              "lease_id": leaseID,
            ]
            if let deviceIdsJson = deviceIdsJson,
               let dData = deviceIdsJson.data(using: .utf8),
               let ids = try? JSONSerialization.jsonObject(with: dData) as? [String],
               !ids.isEmpty {
              commitReq["device_ids"] = ids
            }
            let reqData = try JSONSerialization.data(withJSONObject: commitReq)
            let reqJSON = String(data: reqData, encoding: .utf8) ?? "{}"
            let subJSON = try callGoString { eng.commitContent(nil, reqJSON: reqJSON, error: $0) }
            var payload: [String: Any] = ["cancelled": false]
            if let subData = subJSON.data(using: .utf8),
               let subObj = try? JSONSerialization.jsonObject(with: subData) {
              payload["submission"] = subObj
            }
            let out = try JSONSerialization.data(withJSONObject: payload)
            promise.resolve(String(data: out, encoding: .utf8) ?? "{\"cancelled\":false}")
          } catch let err as UnicModuleException {
            promise.reject(err)
          } catch {
            promise.reject(UnicModuleException(formatGoError(error)))
          }
        }
      }
    }

    AsyncFunction("resendHistoryItem") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.resendHistoryItem(nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("listReceived") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listReceived($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("dismissReceived") { (receiptID: String) in
      do {
        try self.requireEngine().dismissReceived(receiptID)
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("applyReceivedToClipboard") { (receiptID: String) -> String in
      do {
        let eng = try self.requireEngine()
        let recvJSON = try callGoString { eng.listReceived($0) }
        guard let recvData = recvJSON.data(using: .utf8),
              let list = try? JSONSerialization.jsonObject(with: recvData) as? [[String: Any]],
              let entry = list.first(where: { ($0["receipt_id"] as? String) == receiptID }),
              let content = entry["content"] as? [String: Any],
              let parts = content["parts"] as? [[String: Any]] else {
          throw UnicModuleException("[unic:not_found] {\"code\":\"not_found\",\"message\":\"received item not found\"}")
        }

        let expectedEpoch = self.observationEpoch
        let expectedSeq = String(self.observationSequence)
        let beginPayload: [String: Any] = [
          "receipt_id": receiptID,
          "expected_epoch": expectedEpoch,
          "expected_sequence": expectedSeq,
        ]
        let beginData = try JSONSerialization.data(withJSONObject: beginPayload)
        let beginJSON = String(data: beginData, encoding: .utf8) ?? "{}"
        let ticketJSON = try callGoString { eng.beginApply(beginJSON, error: $0) }
        guard let tData = ticketJSON.data(using: .utf8),
              let ticket = try JSONSerialization.jsonObject(with: tData) as? [String: Any],
              let token = ticket["token"] as? String,
              let leaseID = ticket["lease_id"] as? String else {
          throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"invalid apply ticket\"}")
        }

        var reported = false
        defer {
          if !reported {
            self.reportApplyFailure(eng, token: token, errorCode: "internal")
          }
          try? eng.releaseContent(leaseID)
        }

        var applied = false
        if let imgPart = parts.first(where: { ($0["kind"] as? String) == "representation" && (($0["mime"] as? String)?.hasPrefix("image/") == true) }),
           let partID = imgPart["id"] as? String {
          let cacheDir = try self.cacheDirectoryPath()
          let dest = URL(fileURLWithPath: cacheDir, isDirectory: true).appendingPathComponent("apply-recv-\(UUID().uuidString).img")
          var written: Int64 = 0
          try eng.exportPart(toFile: leaseID, partID: partID, destPath: dest.path, ret0_: &written)
          defer { try? FileManager.default.removeItem(at: dest) }
          if let imgData = try? Data(contentsOf: dest), let image = UIImage(data: imgData) {
            DispatchQueue.main.sync {
              UIPasteboard.general.image = image
            }
            applied = true
          }
        } else if let txtPart = parts.first(where: { ($0["kind"] as? String) == "representation" && (($0["mime"] as? String) == "text/plain" || ($0["mime"] as? String) == "text/html") }),
                  let partID = txtPart["id"] as? String {
          let textJSON = try callGoString { eng.readLeasedText(leaseID, partID: partID, maxBytes: 4_194_304, error: $0) }
          if let pData = textJSON.data(using: .utf8),
             let pObj = try? JSONSerialization.jsonObject(with: pData) as? [String: Any],
             let text = pObj["text"] as? String,
             !text.isEmpty {
            DispatchQueue.main.sync {
              UIPasteboard.general.string = text
            }
            applied = true
          }
        }

        let afterChangeCount: Int = DispatchQueue.main.sync { UIPasteboard.general.changeCount }
        if !applied {
          self.reportApplyFailure(eng, token: token, errorCode: "unsupported")
          reported = true
          throw UnicModuleException("[unic:unsupported] {\"code\":\"unsupported\",\"message\":\"received content cannot be applied to clipboard\"}")
        }

        self.lastAppliedChangeCount = afterChangeCount
        self.lastProbedChangeCount = afterChangeCount

        let reportPayload: [String: Any]
        if expectedEpoch.isEmpty {
          reportPayload = [
            "token": token,
            "outcome": "applied",
            "epoch": "",
            "sequence": "0",
          ]
        } else {
          let nextSeq = self.observationSequence + 1
          reportPayload = [
            "token": token,
            "outcome": "applied",
            "baseline_lease_id": leaseID,
            "epoch": expectedEpoch,
            "sequence": String(nextSeq),
          ]
          self.observationSequence = nextSeq
        }
        let reportData = try JSONSerialization.data(withJSONObject: reportPayload)
        try eng.reportApplyResult(String(data: reportData, encoding: .utf8) ?? "{}")
        reported = true
        return "{\"applied\":true}"
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("applyHistoryToClipboard") { (historyID: String) -> String in
      do {
        let eng = try self.requireEngine()
        let expectedEpoch = self.observationEpoch
        let expectedSeq = String(self.observationSequence)
        let ticketJSON = try callGoString {
          eng.beginApplyHistory(historyID, expectedEpoch: expectedEpoch, expectedSequence: expectedSeq, userRestore: true, error: $0)
        }
        guard let tData = ticketJSON.data(using: .utf8),
              let ticket = try JSONSerialization.jsonObject(with: tData) as? [String: Any],
              let token = ticket["token"] as? String,
              let leaseID = ticket["lease_id"] as? String else {
          throw UnicModuleException("[unic:internal] {\"code\":\"internal\",\"message\":\"invalid apply ticket\"}")
        }

        var reported = false
        defer {
          if !reported {
            self.reportApplyFailure(eng, token: token, errorCode: "internal")
          }
          try? eng.releaseContent(leaseID)
        }

        let itemJSON = try callGoString { eng.getHistoryItem(historyID, error: $0) }
        var applied = false
        if let itemData = itemJSON.data(using: .utf8),
           let itemObj = try? JSONSerialization.jsonObject(with: itemData) as? [String: Any],
           let kind = itemObj["kind"] as? String,
           kind == "image" {
          let cacheDir = try self.cacheDirectoryPath()
          let dest = URL(fileURLWithPath: cacheDir, isDirectory: true).appendingPathComponent("apply-\(UUID().uuidString).img")
          _ = try callGoString {
            eng.exportHistoryPart(toFile: historyID, mimePrefix: "image/", destPath: dest.path, error: $0)
          }
          defer { try? FileManager.default.removeItem(at: dest) }
          if let imgData = try? Data(contentsOf: dest), let image = UIImage(data: imgData) {
            DispatchQueue.main.sync {
              UIPasteboard.general.image = image
            }
            applied = true
          }
        } else {
          let previewJSON = try callGoString { eng.readHistoryFullText(historyID, maxBytes: 4_194_304, error: $0) }
          if let pData = previewJSON.data(using: .utf8),
             let pObj = try? JSONSerialization.jsonObject(with: pData) as? [String: Any],
             let text = pObj["text"] as? String,
             !text.isEmpty {
            DispatchQueue.main.sync {
              UIPasteboard.general.string = text
            }
            applied = true
          }
        }

        let afterChangeCount: Int = DispatchQueue.main.sync { UIPasteboard.general.changeCount }
        if !applied {
          self.reportApplyFailure(eng, token: token, errorCode: "unsupported")
          reported = true
          throw UnicModuleException("[unic:unsupported] {\"code\":\"unsupported\",\"message\":\"clipboard content could not be applied\"}")
        }

        self.lastAppliedChangeCount = afterChangeCount
        self.lastProbedChangeCount = afterChangeCount

        let reportPayload: [String: Any]
        if expectedEpoch.isEmpty {
          reportPayload = [
            "token": token,
            "outcome": "applied",
            "epoch": "",
            "sequence": "0",
          ]
        } else {
          let nextSeq = self.observationSequence + 1
          reportPayload = [
            "token": token,
            "outcome": "applied",
            "baseline_lease_id": leaseID,
            "epoch": expectedEpoch,
            "sequence": String(nextSeq),
          ]
          self.observationSequence = nextSeq
        }
        let reportData = try JSONSerialization.data(withJSONObject: reportPayload)
        try eng.reportApplyResult(String(data: reportData, encoding: .utf8) ?? "{}")
        reported = true
        return "{\"applied\":true}"
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("readSystemClipboardAndCommit") { () -> String in
      let eng = try self.requireEngine()
      return try self.observeCurrentPasteboardChange(eng)
    }.runOnQueue(engineQueue)

    // ---- Actions ----

    AsyncFunction("listActions") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listActions($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("runAction") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.runAction(nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    // ---- Pairing & Devices ----

    AsyncFunction("createPairingCode") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.createPairingCode(nil, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("getPairingCode") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getPairingCode($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("cancelPairingCode") { () in
      do {
        try self.requireEngine().cancelPairingCode()
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("exportPairingTicket") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.exportPairingTicket(nil, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("joinByCode") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.join(byCode: nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("joinByTicket") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.join(byTicket: nil, reqJSON: reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("getOperation") { (opID: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.getOperation(opID, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("listDevices") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listDevices($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("approveDevice") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.approveDevice(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("renameDevice") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.renameDevice(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("setBindingSync") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.setBindingSync(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("unpairDevice") { (deviceID: String, expectedRevision: Int64) in
      do {
        try self.requireEngine().unpairDevice(deviceID, expectedRevision: expectedRevision)
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)

    AsyncFunction("listCandidates") { () -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.listCandidates($0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("pairCandidate") { (reqJSON: String) -> String in
      let eng = try self.requireEngine()
      return try callGoString { eng.pairCandidate(reqJSON, error: $0) }
    }.runOnQueue(engineQueue)

    AsyncFunction("ignoreCandidate") { (deviceID: String, ignored: Bool) in
      do {
        try self.requireEngine().ignoreCandidate(deviceID, ignored: ignored)
      } catch let err as UnicModuleException {
        throw err
      } catch {
        throw UnicModuleException(formatGoError(error))
      }
    }.runOnQueue(engineQueue)
  }
}
