import UIKit
import LinkPresentation
import CryptoKit

struct URLCardMetadata {
    var title: String?
    var ogImage: UIImage?
}

// MARK: - Cache (memory + disk)

@MainActor
final class URLMetadataCache {
    static let shared = URLMetadataCache()

    private var memory: [String: URLCardMetadata] = [:]
    private var inFlight: Set<String> = []
    private let diskDir: URL

    private init() {
        let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        diskDir = caches.appendingPathComponent("URLMetadata", isDirectory: true)
        try? FileManager.default.createDirectory(at: diskDir, withIntermediateDirectories: true)
    }

    func fetch(for url: URL) async -> URLCardMetadata {
        let key = url.absoluteString

        if let hit = memory[key] { return hit }
        if let disk = loadDisk(key: key) {
            memory[key] = disk
            return disk
        }

        guard !inFlight.contains(key) else { return URLCardMetadata() }
        inFlight.insert(key)
        defer { inFlight.remove(key) }

        let result = await URLMetadataFetcher.fetch(url: url)
        memory[key] = result
        saveDisk(key: key, metadata: result)
        return result
    }

    // MARK: Disk

    private func diskHash(_ key: String) -> String {
        SHA256.hash(data: Data(key.utf8))
            .prefix(16)
            .map { String(format: "%02x", $0) }
            .joined()
    }

    private func loadDisk(key: String) -> URLCardMetadata? {
        let h = diskHash(key)
        let metaURL = diskDir.appendingPathComponent("\(h).json")
        guard let data = try? Data(contentsOf: metaURL),
              let disk = try? JSONDecoder().decode(DiskEntry.self, from: data)
        else { return nil }

        var meta = URLCardMetadata(title: disk.title)
        if disk.hasImage {
            let imgURL = diskDir.appendingPathComponent("\(h).jpg")
            if let imgData = try? Data(contentsOf: imgURL) {
                meta.ogImage = UIImage(data: imgData)
            }
        }
        return meta
    }

    private func saveDisk(key: String, metadata: URLCardMetadata) {
        let h = diskHash(key)
        let entry = DiskEntry(title: metadata.title, hasImage: metadata.ogImage != nil)
        if let data = try? JSONEncoder().encode(entry) {
            try? data.write(to: diskDir.appendingPathComponent("\(h).json"))
        }
        if let image = metadata.ogImage,
           let jpeg = image.jpegData(compressionQuality: 0.8) {
            try? jpeg.write(to: diskDir.appendingPathComponent("\(h).jpg"))
        }
    }

    private struct DiskEntry: Codable {
        var title: String?
        var hasImage: Bool
    }
}

// MARK: - Network fetcher

enum URLMetadataFetcher {
    static func fetch(url: URL) async -> URLCardMetadata {
        let provider = LPMetadataProvider()
        provider.shouldFetchSubresources = true
        do {
            let metadata: LPLinkMetadata = try await withCheckedThrowingContinuation { continuation in
                provider.startFetchingMetadata(for: url) { meta, error in
                    if let error { continuation.resume(throwing: error) }
                    else if let meta { continuation.resume(returning: meta) }
                    else { continuation.resume(throwing: URLError(.badServerResponse)) }
                }
            }
            let title = metadata.title
            var image: UIImage?
            if let imageProvider = metadata.imageProvider {
                image = await loadImage(from: imageProvider)
            }
            return URLCardMetadata(title: title, ogImage: image)
        } catch {
            return URLCardMetadata()
        }
    }

    private static func loadImage(from provider: NSItemProvider) async -> UIImage? {
        await withCheckedContinuation { continuation in
            provider.loadObject(ofClass: UIImage.self) { object, _ in
                continuation.resume(returning: object as? UIImage)
            }
        }
    }
}
