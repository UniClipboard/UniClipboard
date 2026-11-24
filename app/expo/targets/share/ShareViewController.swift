import UIKit
import UniformTypeIdentifiers

final class ShareViewController: UIViewController {
  private static let maxBytes = 256 * 1024 * 1024

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .systemBackground
    let indicator = UIActivityIndicatorView(style: .large)
    indicator.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(indicator)
    NSLayoutConstraint.activate([
      indicator.centerXAnchor.constraint(equalTo: view.centerXAnchor),
      indicator.centerYAnchor.constraint(equalTo: view.centerYAnchor),
    ])
    indicator.startAnimating()
  }

  override func viewDidAppear(_ animated: Bool) {
    super.viewDidAppear(animated)
    guard let context = extensionContext else { return }
    guard !staging else { return }
    staging = true
    Task {
      do {
        try await stage(context)
        context.completeRequest(returningItems: nil)
      } catch {
        context.cancelRequest(withError: error)
      }
    }
  }

  private var staging = false

  private func stage(_ context: NSExtensionContext) async throws {
    let fs = FileManager.default
    guard let extensionID = Bundle.main.bundleIdentifier, extensionID.hasSuffix(".Share"),
          let group = fs.containerURL(forSecurityApplicationGroupIdentifier: "group." + extensionID.dropLast(6) + ".incoming-share")
    else {
      throw CocoaError(.fileNoSuchFile)
    }
    let root = group.appendingPathComponent("incoming-share", isDirectory: true)
    try fs.createDirectory(at: root, withIntermediateDirectories: true)
    let providers = (context.inputItems as? [NSExtensionItem] ?? []).flatMap { $0.attachments ?? [] }
    guard !providers.isEmpty && providers.count <= 20 else { throw CocoaError(.fileWriteOutOfSpace) }

    let queued = try fs.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)
      .filter { UUID(uuidString: $0.lastPathComponent) != nil }
    guard queued.count + providers.count <= 64 else { throw CocoaError(.fileWriteOutOfSpace) }
    let occupied = queued.reduce(0) { sum, dir in
      let values = try? dir.appendingPathComponent("payload").resourceValues(forKeys: [.fileSizeKey])
      return sum + (values?.fileSize ?? 256 * 1024)
    }
    var remaining = max(0, Self.maxBytes - occupied)
    var saved = 0
    for provider in providers {
      let staging = root.appendingPathComponent("staging-" + UUID().uuidString, isDirectory: true)
      try fs.createDirectory(at: staging, withIntermediateDirectories: false)
      defer { try? fs.removeItem(at: staging) }
      do {
        let entry: [String: String]
        let types = provider.registeredTypeIdentifiers.compactMap { UTType($0) }
        let concreteFile = types.first { $0.conforms(to: .data) && !$0.conforms(to: .text) && !$0.conforms(to: .url) }
        let fileType = concreteFile ?? (provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) ? .fileURL : nil)
        if let type = fileType {
          let copied = try await copy(provider, type: type, to: staging.appendingPathComponent("payload"), limit: remaining)
          remaining -= copied.size
          let sourceExtension = URL(fileURLWithPath: copied.name).pathExtension
          let payloadType = type == .fileURL ? UTType(filenameExtension: sourceExtension) : type
          let original = provider.suggestedName.flatMap { $0.isEmpty ? nil : $0 }
            ?? (type == .fileURL ? copied.name : "shared-file")
          var name = URL(fileURLWithPath: original).lastPathComponent
          if URL(fileURLWithPath: name).pathExtension.isEmpty, let ext = payloadType?.preferredFilenameExtension {
            name += "." + ext
          }
          entry = ["kind": "file", "name": name, "mime": payloadType?.preferredMIMEType ?? "application/octet-stream"]
        } else if let value = try await loadText(provider), !value.isEmpty {
          let length = value.utf8.count
          guard length <= 256 * 1024 && length <= remaining else { throw CocoaError(.fileWriteOutOfSpace) }
          remaining -= length
          entry = ["kind": "text", "text": value]
        } else {
          continue
        }
        try JSONSerialization.data(withJSONObject: entry).write(to: staging.appendingPathComponent("entry.json"), options: .atomic)
        try fs.moveItem(at: staging, to: root.appendingPathComponent(UUID().uuidString.lowercased(), isDirectory: true))
        saved += 1
      } catch {
        if providers.count == 1 { throw error }
      }
    }
    guard saved > 0 else { throw CocoaError(.fileReadUnsupportedScheme) }
  }

  private func copy(_ provider: NSItemProvider, type: UTType, to destination: URL, limit: Int) async throws -> (size: Int, name: String) {
    try await withCheckedThrowingContinuation { continuation in
      let copyFile: (URL?, (any Error)?) -> Void = { url, error in
        do {
          guard let url else { throw error ?? CocoaError(.fileReadNoSuchFile) }
          let scoped = url.startAccessingSecurityScopedResource()
          defer { if scoped { url.stopAccessingSecurityScopedResource() } }
          let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
          guard values.isRegularFile == true, let size = values.fileSize, size <= limit else {
            throw CocoaError(.fileWriteOutOfSpace)
          }
          try FileManager.default.copyItem(at: url, to: destination)
          continuation.resume(returning: (size, url.lastPathComponent))
        } catch {
          continuation.resume(throwing: error)
        }
      }
      if type == .fileURL {
        provider.loadItem(forTypeIdentifier: type.identifier, options: nil) { item, error in
          copyFile(item as? URL, error)
        }
      } else {
        provider.loadFileRepresentation(forTypeIdentifier: type.identifier, completionHandler: copyFile)
      }
    }
  }

  private func loadText(_ provider: NSItemProvider) async throws -> String? {
    let type = provider.hasItemConformingToTypeIdentifier(UTType.url.identifier) ? UTType.url : UTType.plainText
    guard provider.hasItemConformingToTypeIdentifier(type.identifier) else { return nil }
    return try await withCheckedThrowingContinuation { continuation in
      provider.loadItem(forTypeIdentifier: type.identifier, options: nil) { item, error in
        if let error {
          continuation.resume(throwing: error)
        } else {
          let value = (item as? URL)?.absoluteString ?? (item as? String)
          continuation.resume(returning: value)
        }
      }
    }
  }
}
