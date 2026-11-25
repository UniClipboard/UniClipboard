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
    let summaries: [[String: String]] = ordered.compactMap { url in
      let id = url.lastPathComponent
      guard let dir = item(id),
            let data = try? Data(contentsOf: dir.appendingPathComponent("entry.json")),
            let entry = (try? JSONSerialization.jsonObject(with: data)) as? [String: String],
            let kind = entry["kind"],
            let label = (kind == "text" ? entry["text"] : entry["name"]) else { return nil }
      return ["id": id, "kind": kind, "label": String(label.prefix(160))]
    }
    return String(decoding: try JSONSerialization.data(withJSONObject: summaries), as: UTF8.self)
  }

  func remove(_ id: String) throws {
    guard let dir = item(id), let root else { return }
    let trash = root.appendingPathComponent("removed-\(id)", isDirectory: true)
    try fileManager.moveItem(at: dir, to: trash)
    try? fileManager.removeItem(at: trash)
  }
}
