import UIKit

@MainActor
final class ImageThumbnailCache {
    static let shared = ImageThumbnailCache()

    private var memory: [String: UIImage] = [:]
    private var inFlight: Set<String> = []
    private let diskDir: URL
    private let thumbSize = CGSize(width: 400, height: 400)

    private init() {
        let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        diskDir = caches.appendingPathComponent("ImageThumbnails", isDirectory: true)
        try? FileManager.default.createDirectory(at: diskDir, withIntermediateDirectories: true)
    }

    func cached(forHash hash: String) -> UIImage? {
        let key = hash.uppercased()
        if let hit = memory[key] { return hit }
        if let disk = loadDisk(key: key) {
            memory[key] = disk
            return disk
        }
        return nil
    }

    func fetch(
        forHash hash: String,
        using loader: @escaping () async throws -> Data
    ) async -> UIImage? {
        let key = hash.uppercased()
        if let hit = memory[key] { return hit }
        if let disk = loadDisk(key: key) {
            memory[key] = disk
            return disk
        }
        guard !inFlight.contains(key) else { return nil }
        inFlight.insert(key)
        defer { inFlight.remove(key) }

        do {
            let bytes = try await loader()
            guard let full = UIImage(data: bytes) else { return nil }
            let thumb = full.preparingThumbnail(of: thumbSize) ?? full
            memory[key] = thumb
            saveDisk(key: key, image: thumb)
            return thumb
        } catch {
            return nil
        }
    }

    // MARK: - Disk

    private func loadDisk(key: String) -> UIImage? {
        let url = diskDir.appendingPathComponent("\(key).jpg")
        guard let data = try? Data(contentsOf: url) else { return nil }
        return UIImage(data: data)
    }

    private func saveDisk(key: String, image: UIImage) {
        guard let jpeg = image.jpegData(compressionQuality: 0.85) else { return }
        try? jpeg.write(to: diskDir.appendingPathComponent("\(key).jpg"))
    }
}
