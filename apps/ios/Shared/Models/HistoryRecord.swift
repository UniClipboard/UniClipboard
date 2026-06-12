import Foundation

/// On-the-wire history record. Spec: docs/SYNC_PROTOCOL.md §3.6.
///
/// Returned by §2.7 (`POST /api/history/query`), §2.8 (`GET /api/history/<id>`),
/// and §2.10 (PATCH reply). Accepted by §2.9 in **multipart form**, not JSON —
/// don't reuse this type's `encode(to:)` for upload payloads; the multipart
/// shape is constructed field-by-field by the network layer.
///
/// Like `Clipboard`, decoders MUST tolerate missing/unknown fields. Only
/// `hash` and `type` are required; everything else has a sensible default
/// (timestamps treated as unknown, flags `false`).
public struct HistoryRecord: Codable, Equatable, Hashable, Identifiable, Sendable {
    public var hash: String
    public var type: Clipboard.Kind
    public var text: String?
    public var hasData: Bool
    public var size: Int?
    public var createTime: Date?
    public var lastModified: Date?
    public var lastAccessed: Date?
    public var starred: Bool
    public var pinned: Bool
    public var version: Int?
    /// Soft-delete tombstone — the **read** name (§3.6). The PATCH update
    /// body uses `isDelete` (no trailing `d`); that asymmetry lives on a
    /// separate `HistoryRecordUpdate` DTO so it can't leak here.
    public var isDeleted: Bool

    public init(
        hash: String,
        type: Clipboard.Kind,
        text: String? = nil,
        hasData: Bool = false,
        size: Int? = nil,
        createTime: Date? = nil,
        lastModified: Date? = nil,
        lastAccessed: Date? = nil,
        starred: Bool = false,
        pinned: Bool = false,
        version: Int? = nil,
        isDeleted: Bool = false
    ) {
        self.hash = hash
        self.type = type
        self.text = text
        self.hasData = hasData
        self.size = size
        self.createTime = createTime
        self.lastModified = lastModified
        self.lastAccessed = lastAccessed
        self.starred = starred
        self.pinned = pinned
        self.version = version
        self.isDeleted = isDeleted
    }

    /// `Identifiable` conformance: server URLs in §2.8 / §2.11 address a
    /// record by `<type>-<hash>`. SwiftUI lists keying off `\.id` get the
    /// same composite key, so a single record stays diff-stable even as
    /// flags flip.
    public var id: String { Self.profileId(type: type, hash: hash) }

    public static func profileId(type: Clipboard.Kind, hash: String) -> String {
        "\(type.rawValue)-\(hash)"
    }

    private enum CodingKeys: String, CodingKey {
        case hash, type, text, hasData, size
        case createTime, lastModified, lastAccessed
        case starred, pinned, version, isDeleted
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        hash         = try c.decode(String.self, forKey: .hash)
        type         = try c.decode(Clipboard.Kind.self, forKey: .type)
        text         = try c.decodeIfPresent(String.self, forKey: .text)
        hasData      = try c.decodeIfPresent(Bool.self, forKey: .hasData) ?? false
        size         = try c.decodeIfPresent(Int.self,  forKey: .size)
        createTime   = try Self.decodeISODate(c, forKey: .createTime)
        lastModified = try Self.decodeISODate(c, forKey: .lastModified)
        lastAccessed = try Self.decodeISODate(c, forKey: .lastAccessed)
        starred      = try c.decodeIfPresent(Bool.self, forKey: .starred) ?? false
        pinned       = try c.decodeIfPresent(Bool.self, forKey: .pinned)  ?? false
        version      = try c.decodeIfPresent(Int.self,  forKey: .version)
        isDeleted    = try c.decodeIfPresent(Bool.self, forKey: .isDeleted) ?? false
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(hash, forKey: .hash)
        try c.encode(type, forKey: .type)
        try c.encodeIfPresent(text, forKey: .text)
        // hasData / starred / pinned / isDeleted default to false. Encoding
        // them unconditionally keeps the wire shape predictable for tests
        // and aligns with the Android client, which emits all four flags
        // explicitly on its outbound JSON.
        try c.encode(hasData, forKey: .hasData)
        try c.encodeIfPresent(size, forKey: .size)
        try Self.encodeISODate(&c, createTime,   forKey: .createTime)
        try Self.encodeISODate(&c, lastModified, forKey: .lastModified)
        try Self.encodeISODate(&c, lastAccessed, forKey: .lastAccessed)
        try c.encode(starred, forKey: .starred)
        try c.encode(pinned,  forKey: .pinned)
        try c.encodeIfPresent(version, forKey: .version)
        try c.encode(isDeleted, forKey: .isDeleted)
    }

    /// Tolerant ISO-8601 decoder: accepts both `…Z` and `…+00:00` flavors,
    /// and treats fractional seconds as optional. The Android wire uses
    /// fractional-seconds + `Z` (`2026-05-17T16:43:21.420Z`); some
    /// hand-rolled servers truncate. Both parse here.
    private static func decodeISODate(
        _ c: KeyedDecodingContainer<CodingKeys>,
        forKey key: CodingKeys
    ) throws -> Date? {
        guard let raw = try c.decodeIfPresent(String.self, forKey: key),
              !raw.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return nil }
        if let d = HistoryRecord.fractionalISOFormatter.date(from: raw) { return d }
        if let d = HistoryRecord.plainISOFormatter.date(from: raw) { return d }
        // Last-resort: ISO8601DateFormatter with broader options doesn't
        // exist as a single combo, so we fail loud rather than guess —
        // the spec is ISO-8601, callers should send compliant strings.
        throw DecodingError.dataCorruptedError(
            forKey: key,
            in: c,
            debugDescription: "Not a recognized ISO-8601 timestamp: \(raw)"
        )
    }

    private static func encodeISODate(
        _ c: inout KeyedEncodingContainer<CodingKeys>,
        _ date: Date?,
        forKey key: CodingKeys
    ) throws {
        guard let date else { return }
        try c.encode(HistoryRecord.fractionalISOFormatter.string(from: date), forKey: key)
    }

    private static let fractionalISOFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let plainISOFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()
}
