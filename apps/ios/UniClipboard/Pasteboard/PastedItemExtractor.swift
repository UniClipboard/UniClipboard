import Foundation
import UniformTypeIdentifiers
import UIKit

/// Turns the `[NSItemProvider]` handed to us by SwiftUI's `PasteButton`
/// into a pushable `DeviceClipboardSnapshot`. Because the user tapped the
/// system paste control, iOS grants content access here WITHOUT the "Allow
/// Paste" prompt — this is the whole point of the consent-push path.
///
/// Priority mirrors `ShareItemExtractor` (the share-extension equivalent):
/// URL > text > image. A web URL is pushed as its absolute string (the
/// highest-signal text on iOS); images probe PNG > HEIC > JPEG > GIF to
/// match `DevicePasteboardObserver.imageUTIPriority`. Arbitrary files are
/// out of scope — the Home `PasteButton` only advertises text/url/image
/// content types, and `pushReturningEntry` only pushes `.text`/`.image`.
///
/// Returns `nil` when nothing usable could be extracted (the source app
/// advertised a UTI it couldn't fulfill, or the providers were empty).
/// Loaders swallow per-item errors and fall through rather than throwing —
/// a single dud representation shouldn't abort the whole paste.
enum PastedItemExtractor {

    /// Supported content types to advertise on the `PasteButton`. The
    /// system uses these both to enable/disable the control and to filter
    /// which representations it hands back.
    static let supportedContentTypes: [UTType] = [
        .url, .plainText, .text, .png, .heic, .jpeg, .gif, .image,
    ]

    static func snapshot(from providers: [NSItemProvider]) async -> DeviceClipboardSnapshot? {
        // Priority 1 — non-file URL (Safari "copy page link", etc.).
        for p in providers where p.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
            if let url = await loadURL(p), !url.isFileURL {
                let (clip, payload) = Clipboard.publishText(url.absoluteString)
                return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
            }
        }

        // Priority 2 — plain text.
        for p in providers {
            for uti in [UTType.plainText.identifier, UTType.text.identifier]
            where p.hasItemConformingToTypeIdentifier(uti) {
                if let s = await loadString(p, uti: uti),
                   !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    let (clip, payload) = Clipboard.publishText(s)
                    return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
                }
            }
        }

        // Priority 3 — image. PNG (screenshots) > HEIC (Photos) > JPEG > GIF.
        for p in providers {
            for (uti, ext) in [
                (UTType.png.identifier,  "png"),
                (UTType.heic.identifier, "heic"),
                (UTType.jpeg.identifier, "jpg"),
                (UTType.gif.identifier,  "gif"),
            ] where p.hasItemConformingToTypeIdentifier(uti) {
                if let bytes = await loadBytes(p, uti: uti), !bytes.isEmpty {
                    let (clip, payload) = Clipboard.publishImage(bytes: bytes, ext: ext)
                    return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
                }
            }
            // Fallback: an image UTI we didn't explicitly probe — load as PNG.
            if p.hasItemConformingToTypeIdentifier(UTType.image.identifier),
               let bytes = await loadBytes(p, uti: UTType.image.identifier), !bytes.isEmpty {
                let (clip, payload) = Clipboard.publishImage(bytes: bytes, ext: "png")
                return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
            }
        }

        return nil
    }

    // MARK: - NSItemProvider async wrappers (nil on failure, never throw)
    //
    // Critical difference from `ShareItemExtractor`: the providers
    // `PasteButton` hands us are `UIPasteboard.general.itemProviders`, and
    // for text/URL those return a *file URL into the Pasteboard cache* from
    // `loadItem(forTypeIdentifier:)` — not a `String`, `Data`, or web `URL`.
    // `loadItem` alone therefore yields nil for the two most common kinds
    // (copied text and links), so the consent push silently no-ops. The
    // high-level `loadObject(ofClass:)` coerces every storage form (in-memory
    // value, raw data, or cached file) into the object, so we try it first
    // and keep `loadItem` as the fallback for share-sheet-style providers.

    private static func loadURL(_ p: NSItemProvider) async -> URL? {
        if p.canLoadObject(ofClass: NSURL.self) {
            if let url = await withCheckedContinuation({ (cont: CheckedContinuation<URL?, Never>) in
                _ = p.loadObject(ofClass: NSURL.self) { value, _ in
                    cont.resume(returning: value as? URL)
                }
            }) {
                return url
            }
        }
        return await withCheckedContinuation { cont in
            p.loadItem(forTypeIdentifier: UTType.url.identifier, options: nil) { value, _ in
                cont.resume(returning: value as? URL)
            }
        }
    }

    private static func loadString(_ p: NSItemProvider, uti: String) async -> String? {
        if p.canLoadObject(ofClass: NSString.self) {
            if let s = await withCheckedContinuation({ (cont: CheckedContinuation<String?, Never>) in
                _ = p.loadObject(ofClass: NSString.self) { value, _ in
                    cont.resume(returning: value as? String)
                }
            }) {
                return s
            }
        }
        return await withCheckedContinuation { cont in
            p.loadItem(forTypeIdentifier: uti, options: nil) { value, _ in
                if let s = value as? String { cont.resume(returning: s); return }
                if let url = value as? URL {
                    if !url.isFileURL { cont.resume(returning: url.absoluteString); return }
                    if let s = try? String(contentsOf: url, encoding: .utf8) {
                        cont.resume(returning: s); return
                    }
                }
                if let data = value as? Data, let s = String(data: data, encoding: .utf8) {
                    cont.resume(returning: s); return
                }
                cont.resume(returning: nil)
            }
        }
    }

    private static func loadBytes(_ p: NSItemProvider, uti: String) async -> Data? {
        await withCheckedContinuation { cont in
            p.loadItem(forTypeIdentifier: uti, options: nil) { value, _ in
                if let data = value as? Data { cont.resume(returning: data); return }
                if let url = value as? URL, url.isFileURL {
                    cont.resume(returning: try? Data(contentsOf: url)); return
                }
                if let image = value as? UIImage, let data = image.pngData() {
                    cont.resume(returning: data); return
                }
                cont.resume(returning: nil)
            }
        }
    }
}
