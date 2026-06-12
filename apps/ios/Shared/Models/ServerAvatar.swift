import Foundation

/// Pure-Foundation helpers for representing a `ServerConfig` as a small
/// avatar. Used by:
///   • `Shared/Intents/ServerPersonFactory` to render an `INImage` for
///     `INPerson`, which powers the "share to contact" tile on the system
///     share sheet.
///   • (Eventually) any SwiftUI surface that wants to show a per-server
///     monogram chip.
///
/// The mapping from `ServerConfig` to (`initials`, `hue`) is deterministic
/// — given the same `id`, both fields are stable across launches and
/// devices. That stability is what lets the share-sheet tile stay visually
/// consistent regardless of which process (main app vs. extension) donated
/// the `INInteraction`.
public enum ServerAvatar {
    /// Up to two characters that represent the server in a circular badge.
    ///
    /// Source priority: server name (if non-empty after trim) → URL host
    /// (if parseable) → URL string → `"?"`. The result is uppercased
    /// (locale-aware, so e.g. Turkish dotless-i would survive a real
    /// future locale change correctly).
    public static func initials(for server: ServerConfig) -> String {
        if let name = server.name?.trimmingCharacters(in: .whitespacesAndNewlines),
           !name.isEmpty {
            return initials(from: name)
        }
        if let host = URL(string: server.url)?.host, !host.isEmpty {
            // For a host, the most-distinctive label is the leftmost one
            // (e.g. `nas.example.com` → `nas`). Without a public-suffix
            // list we just take the first dot-segment instead of feeding
            // the whole host through the multi-word splitter, which would
            // otherwise mash "nas" + "example" into "NE".
            let firstLabel = host.split(separator: ".").first.map(String.init) ?? host
            return initials(from: firstLabel)
        }
        let trimmed = server.url.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty { return initials(from: trimmed) }
        return "?"
    }

    /// HSL/HSB hue in [0, 1) derived from `server.id` so the avatar color
    /// stays stable across launches. We keep the full hue wheel and let
    /// the renderer pick fixed saturation/value, so two servers nearly
    /// always look distinct (collisions only at ~1/360 hue resolution).
    ///
    /// Uses FNV-1a 32-bit (not SHA-256) intentionally — we don't need
    /// cryptographic strength here, and FNV is small enough to inline
    /// and read.
    public static func hue(for server: ServerConfig) -> Double {
        let h = fnv1a32(server.id)
        return Double(h) / Double(UInt32.max)
    }

    // MARK: - Internals

    /// Split `s` on whitespace/hyphen/underscore/dot. For ≥ 2 words, take
    /// the first character of the first two. For 1 word, take the first
    /// 1–2 characters. Falls back to `"?"` on an all-separator input.
    static func initials(from s: String) -> String {
        let separators = CharacterSet(charactersIn: " \t\n\r-_.·")
        let parts = s
            .components(separatedBy: separators)
            .filter { !$0.isEmpty }

        if parts.count >= 2 {
            let a = parts[0].first.map(String.init) ?? ""
            let b = parts[1].first.map(String.init) ?? ""
            let joined = (a + b)
            return joined.isEmpty ? "?" : joined.uppercased()
        }
        if let word = parts.first {
            let prefix = word.prefix(2)
            return prefix.isEmpty ? "?" : String(prefix).uppercased()
        }
        return "?"
    }

    /// FNV-1a over the UTF-8 bytes of `s`. Small, fast, deterministic.
    private static func fnv1a32(_ s: String) -> UInt32 {
        var hash: UInt32 = 0x811c9dc5
        for byte in s.utf8 {
            hash ^= UInt32(byte)
            hash = hash &* 0x01000193
        }
        return hash
    }
}
