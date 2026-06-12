import Foundation

/// One row in the Home tab's time-descending clipboard list. A snapshot
/// of a `Clipboard` (§3) with locally-attributed metadata: when this
/// device observed it and which direction it flowed (server → device
/// pull, or device → server push).
///
/// Not part of the SyncClipboard wire protocol — the spec keeps exactly
/// one record on the server for the live clipboard (§2.1) and clients
/// accumulate their own observation log locally. Codable so the App
/// Group's `SettingsStore` can persist it under
/// `AppSettings.PersistenceKey.clipboardHistory`; surviving an app kill
/// is the difference between "feels like a sync client" and "feels like
/// a polling toy".
public struct ClipboardHistoryItem: Identifiable, Hashable, Codable, Sendable {
    public enum Direction: String, Hashable, Codable, Sendable {
        case pulled, pushed, local
    }

    /// Stable across encodings — must be `var` (not `let`) so the Codable
    /// synthesis emits a decoder that reads the stored value rather than
    /// minting a fresh `UUID()` on every round-trip (which would scramble
    /// `Identifiable` semantics in the UI).
    public var id: UUID
    public var entry: Clipboard
    public var timestamp: Date
    public var direction: Direction

    public init(
        id: UUID = UUID(),
        entry: Clipboard,
        timestamp: Date,
        direction: Direction
    ) {
        self.id = id
        self.entry = entry
        self.timestamp = timestamp
        self.direction = direction
    }
}
