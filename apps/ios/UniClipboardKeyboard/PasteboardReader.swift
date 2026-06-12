import Foundation
import UIKit

/// Reads `UIPasteboard.general` for the keyboard's uplink. Text + image only
/// (handoff scope: files go through the Share Extension). Bytes are pulled
/// via `data(forPasteboardType:)` — never `pb.image`, which decodes through
/// `UIImage` and would change the bytes, breaking the §4.2 content hash.
/// Image UTI priority is PNG > HEIC > JPEG > GIF, matching
/// `DevicePasteboardObserver` so device-side and stub-side hashes agree.
///
/// Per the memory cap on keyboard extensions, image bytes are passed straight
/// through to the uploader as `Data` — we never instantiate a `UIImage`.
///
/// The first call after Full Access is granted triggers iOS's per-app
/// "允许粘贴" prompt once; the read returns `nil` until the user allows, then
/// is silent forever after. The model treats a `nil` snapshot as "nothing to
/// push", so a denied/at-prompt read simply skips the uplink this round.
enum PasteboardReader {
    /// Image UTIs in read priority. PNG first (screenshot default + lossless),
    /// HEIC over JPEG (modern Photos), GIF last (rare).
    private static let imageUTIPriority: [(uti: String, ext: String)] = [
        ("public.png", "png"),
        ("public.heic", "heic"),
        ("public.jpeg", "jpg"),
        ("com.compuserve.gif", "gif"),
    ]

    /// Bytes-fresh read. Returns the publishable `Clipboard` + payload bytes,
    /// or `nil` when the pasteboard is empty / unreadable (no Full Access, or
    /// the "允许粘贴" prompt swallowed this read).
    @MainActor
    static func snapshot() -> DeviceClipboardSnapshot? {
        let pb = UIPasteboard.general
        for (uti, ext) in imageUTIPriority {
            if let data = pb.data(forPasteboardType: uti), !data.isEmpty {
                let (clip, payload) = Clipboard.publishImage(bytes: data, ext: ext)
                return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
            }
        }
        if let s = pb.string, !s.isEmpty {
            let (clip, payload) = Clipboard.publishText(s)
            return DeviceClipboardSnapshot(clipboard: clip, payload: payload)
        }
        return nil
    }

    /// Map a normalized image extension back to the UTI used to write bytes
    /// onto `UIPasteboard.general` for the image-downlink "copy" action.
    static func uti(forExt ext: String) -> String {
        switch ext.lowercased() {
        case "png":          return "public.png"
        case "heic", "heif": return "public.heic"
        case "jpg", "jpeg":  return "public.jpeg"
        case "gif":          return "com.compuserve.gif"
        default:             return "public.png"
        }
    }
}
