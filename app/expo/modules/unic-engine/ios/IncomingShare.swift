import Foundation

struct IncomingShare {
  private let fileManager = FileManager.default

  private var root: URL? {
    guard let bundleID = Bundle.main.bundleIdentifier else { return nil }
    return fileManager.containerURL(forSecurityApplicationGroupIdentifier: "group.\(bundleID).incoming-share")?
      .appendingPathComponent("incoming-share", isDirectory: true)
  }

  func item(_ id: String) -> URL? {
    guard UUID(uuidString: id)?.uuidString.lowercased() == id, let root else { return nil }
    let url = root.appendingPathComponent(id, isDirectory: true)
    guard let info = try? url.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey]),
          info.isDirectory == true, info.isSymbolicLink != true else { return nil }
    return url
  }

  func list() throws -> String {
    guard let root else { return "[]" }
    let entries = (try? fileManager.contentsOfDirectory(at: root, includingPropertiesForKeys: [.creationDateKey])) ?? []
    let ordered = entries.sorted { lhs, rhs in
      let left = (try? lhs.resourceValues(forKeys: [.creationDateKey]))?.creationDate ?? .distantPast
      let right = (try? rhs.resourceValues(forKeys: [.creationDateKey]))?.creationDate ?? .distantPast
      return left == right ? lhs.lastPathComponent < rhs.lastPathComponent : left < right
    }
    let ids = ordered.compactMap { url -> String? in
      let id = url.lastPathComponent
      return item(id) == nil ? nil : id
    }
    return String(decoding: try JSONSerialization.data(withJSONObject: ids), as: UTF8.self)
  }

  func remove(_ id: String) throws {
    guard let dir = item(id), let root else { return }
    let trash = root.appendingPathComponent("removed-\(id)", isDirectory: true)
    try fileManager.moveItem(at: dir, to: trash)
    try? fileManager.removeItem(at: trash)
  }
}
