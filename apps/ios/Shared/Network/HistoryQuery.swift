import Foundation
#if canImport(UniClipboardModels)
import UniClipboardModels
#endif

/// Filter parameters for `POST /api/history/query` (spec §2.7).
///
/// All fields optional. The encoder emits only the fields that are
/// non-nil — the spec says missing fields default to "no filter" on the
/// server side, so omission is meaningful (don't send `page=""` etc).
///
/// Lives in the Network target because it owns the multipart encoding
/// convention; the model layer should never see "true"/"false" strings.
public struct HistoryQuery: Sendable, Equatable {
    /// 1-indexed page. Omit to fetch from the start. Empty result page
    /// (returned by `queryHistory`) signals end-of-list.
    public var page: Int?
    /// Strict upper bound on `createTime`. Records where
    /// `createTime < before` are kept.
    public var before: Date?
    /// Inclusive lower bound on `createTime`. Records where
    /// `createTime >= after` are kept.
    public var after: Date?
    /// **Strict** lower bound on `lastModified` — the incremental-sync
    /// primitive. Pass the highest `lastModified` seen across the prior
    /// merged pages to fetch only what changed since.
    public var modifiedAfter: Date?
    /// Bitmask: Text=1, Image=2, File=4, Group=8. Use `15` for "all" or
    /// `12` for "files + groups". Nil = no type filter.
    public var types: Int?
    /// Server-side substring match against the record's `text`.
    public var searchText: String?
    public var starred: Bool?
    /// When `true`, server sorts the result by `lastAccessed` desc
    /// instead of the default `createTime` desc.
    public var sortByLastAccessed: Bool?

    public init(
        page: Int? = nil,
        before: Date? = nil,
        after: Date? = nil,
        modifiedAfter: Date? = nil,
        types: Int? = nil,
        searchText: String? = nil,
        starred: Bool? = nil,
        sortByLastAccessed: Bool? = nil
    ) {
        self.page = page
        self.before = before
        self.after = after
        self.modifiedAfter = modifiedAfter
        self.types = types
        self.searchText = searchText
        self.starred = starred
        self.sortByLastAccessed = sortByLastAccessed
    }

    /// Type bitmask convenience — `text=1, image=2, file=4, group=8`.
    /// Matches the Flutter wire's encoding (§2.7).
    public enum TypeMask: Int {
        case text  = 1
        case image = 2
        case file  = 4
        case group = 8

        public static let all: Int = 15
    }

    /// Build the multipart body for this query. The Network layer's
    /// `queryHistory` plumbs the result into the URLRequest; surfaced as
    /// a method (rather than a private impl detail) so tests can assert
    /// the encoding deterministically without going through MockURLProtocol.
    public func multipartEncoded(boundary: String? = nil) -> MultipartBody {
        var body: MultipartBody = boundary.map(MultipartBody.init(boundary:)) ?? MultipartBody()
        if let page          { body.append(name: "page", value: String(page)) }
        if let before        { body.append(name: "before", value: Self.iso(before)) }
        if let after         { body.append(name: "after", value: Self.iso(after)) }
        if let modifiedAfter { body.append(name: "modifiedAfter", value: Self.iso(modifiedAfter)) }
        if let types         { body.append(name: "types", value: String(types)) }
        if let searchText    { body.append(name: "searchText", value: searchText) }
        if let starred       { body.append(name: "starred", value: starred ? "true" : "false") }
        if let sortByLastAccessed {
            body.append(name: "sortByLastAccessed", value: sortByLastAccessed ? "true" : "false")
        }
        return body
    }

    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func iso(_ date: Date) -> String {
        isoFormatter.string(from: date)
    }
}
