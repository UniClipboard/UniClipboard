import Foundation
import UniformTypeIdentifiers
import UIKit

/// What we actually push to the server, after extracting one attachment
/// from the system share sheet. Mirrors the three publish paths on
/// `Clipboard`: `publishText`, `publishImage`, `publishFile`.
enum ShareItem: Equatable {
    case text(String)
    case image(Data, ext: String)
    case file(name: String, bytes: Data)

    var displayName: String {
        switch self {
        case .text(let s):
            let trimmed = s.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.count <= 80 { return trimmed }
            return String(trimmed.prefix(80)) + "…"
        case .image(_, let ext):  return "image.\(ext)"
        case .file(let name, _):  return name
        }
    }

    var byteCount: Int {
        switch self {
        case .text(let s):              return s.utf8.count
        case .image(let d, _):          return d.count
        case .file(_, let b):           return b.count
        }
    }

    /// Content-free case name for logging/telemetry — never the payload.
    var kindLabel: String {
        switch self {
        case .text:  return "text"
        case .image: return "image"
        case .file:  return "file"
        }
    }
}

/// Pulls one `ShareItem` out of the system share sheet attachments. Tries
/// type identifiers in priority order: URL > text > image > file. The
/// system already filtered to types declared in our `NSExtensionActivationRule`,
/// so the failure mode here is "the source app advertised a UTI it can't
/// fulfill" which we surface to the user as `.noUsableAttachment`.
enum ShareItemError: Error, LocalizedError {
    case noInputItems
    case noUsableAttachment
    case loadFailed(String)

    var errorDescription: String? {
        switch self {
        case .noInputItems:        return String(localized: "没有可分享的内容")
        case .noUsableAttachment:  return String(localized: "暂不支持这种内容")
        case .loadFailed(let s):   return String(localized: "读取分享内容失败: \(s)")
        }
    }
}

enum ShareItemExtractor {
    static func extract(from ctx: ShareExtensionContext) async throws -> ShareItem {
        let providers = ctx.inputItems.flatMap { $0.attachments ?? [] }
        guard !providers.isEmpty else { throw ShareItemError.noInputItems }

        // Priority 1 — public.url: Safari "share this page", Mail attachments
        // (often surfaced as file-url), etc. URL-shaped sharing is by far
        // the highest-signal text on iOS.
        for p in providers where p.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
            if let url = try await loadURL(p) {
                if url.isFileURL {
                    return try await readFileURL(url)
                }
                return .text(url.absoluteString)
            }
        }

        // Priority 2 — plain text. `public.plain-text` is the most common;
        // `public.text` is a parent UTI that some apps register.
        for p in providers {
            for uti in [UTType.plainText.identifier, UTType.text.identifier]
            where p.hasItemConformingToTypeIdentifier(uti) {
                if let s = try await loadString(p, uti: uti) {
                    return .text(s)
                }
            }
        }

        // Priority 3 — image. Photos: HEIC. Screenshots / web: PNG. Old
        // photos / random apps: JPEG. GIF is rare. We probe in this order.
        for p in providers {
            for (uti, ext) in [
                (UTType.png.identifier,  "png"),
                (UTType.heic.identifier, "heic"),
                (UTType.jpeg.identifier, "jpg"),
                (UTType.gif.identifier,  "gif"),
            ] where p.hasItemConformingToTypeIdentifier(uti) {
                if let bytes = try await loadBytes(p, uti: uti) {
                    return .image(bytes, ext: ext)
                }
            }
            // Fallback: any image UTI we didn't explicitly probe — load as
            // PNG (most-decodable fallback ext).
            if p.hasItemConformingToTypeIdentifier(UTType.image.identifier) {
                if let bytes = try await loadBytes(p, uti: UTType.image.identifier) {
                    return .image(bytes, ext: "png")
                }
            }
        }

        // Priority 4 — arbitrary file via Files-app share.
        for p in providers {
            for uti in [UTType.fileURL.identifier, UTType.data.identifier]
            where p.hasItemConformingToTypeIdentifier(uti) {
                if let url = try await loadURL(p), url.isFileURL {
                    return try await readFileURL(url)
                }
                if let bytes = try await loadBytes(p, uti: uti) {
                    let suggestedName = p.suggestedName ?? "file"
                    return .file(name: suggestedName, bytes: bytes)
                }
            }
        }

        throw ShareItemError.noUsableAttachment
    }

    // MARK: - NSItemProvider async wrappers

    private static func loadURL(_ p: NSItemProvider) async throws -> URL? {
        try await withCheckedThrowingContinuation { cont in
            p.loadItem(forTypeIdentifier: UTType.url.identifier, options: nil) { value, err in
                if let err { cont.resume(throwing: ShareItemError.loadFailed("\(err)")); return }
                cont.resume(returning: value as? URL)
            }
        }
    }

    private static func loadString(_ p: NSItemProvider, uti: String) async throws -> String? {
        try await withCheckedThrowingContinuation { cont in
            p.loadItem(forTypeIdentifier: uti, options: nil) { value, err in
                if let err { cont.resume(throwing: ShareItemError.loadFailed("\(err)")); return }
                if let s = value as? String { cont.resume(returning: s); return }
                if let url = value as? URL, !url.isFileURL { cont.resume(returning: url.absoluteString); return }
                if let data = value as? Data, let s = String(data: data, encoding: .utf8) {
                    cont.resume(returning: s); return
                }
                cont.resume(returning: nil)
            }
        }
    }

    /// Reads bytes for the requested UTI. The system delivers payloads two
    /// ways depending on the source — sometimes as a `URL` pointing into
    /// an extension-scoped temp dir (large images, files), sometimes as
    /// in-memory `Data`. We collapse both into bytes.
    private static func loadBytes(_ p: NSItemProvider, uti: String) async throws -> Data? {
        try await withCheckedThrowingContinuation { cont in
            p.loadItem(forTypeIdentifier: uti, options: nil) { value, err in
                if let err { cont.resume(throwing: ShareItemError.loadFailed("\(err)")); return }
                if let data = value as? Data { cont.resume(returning: data); return }
                if let url = value as? URL, url.isFileURL {
                    do {
                        let data = try Data(contentsOf: url)
                        cont.resume(returning: data)
                    } catch {
                        cont.resume(throwing: ShareItemError.loadFailed("\(error)"))
                    }
                    return
                }
                if let image = value as? UIImage, let data = image.pngData() {
                    cont.resume(returning: data); return
                }
                cont.resume(returning: nil)
            }
        }
    }

    private static func readFileURL(_ url: URL) async throws -> ShareItem {
        do {
            let bytes = try Data(contentsOf: url)
            let name = url.lastPathComponent
            // If it's an image extension, surface as image so the server
            // stores it under `Image` kind and the main app applies it to
            // the pasteboard. Otherwise it's a generic file.
            let ext = url.pathExtension.lowercased()
            if ["png", "jpg", "jpeg", "heic", "heif", "gif"].contains(ext) {
                let normalized = ext == "jpeg" ? "jpg" : (ext == "heif" ? "heic" : ext)
                return .image(bytes, ext: normalized)
            }
            return .file(name: name, bytes: bytes)
        } catch {
            throw ShareItemError.loadFailed("\(error)")
        }
    }
}
