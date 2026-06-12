import Foundation

/// Typed error surface for SyncClipboard protocol calls.
/// Spec: docs/SYNC_PROTOCOL.md §6.
public struct SyncError: Error, Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        case invalidURL
        case connectTimeout
        case receiveTimeout
        case networkUnreachable
        case authFailed
        case notFound
        case protocolError(Int)
        case serverError(Int)
        case decodingFailed
        /// §4.4 — downloaded payload's SHA-256 didn't match the metadata
        /// `hash`. Purely client-side; not produced by `mapHTTPStatus`.
        case hashMismatch
        /// The caller deliberately aborted the request — a network-path
        /// change or live-URL flip invalidated the URL the request was
        /// built against (§5.3). Not a server or connectivity failure:
        /// the sync engine treats it as a silent no-op (no backoff, no
        /// state flip), unlike every other kind.
        case cancelled
    }

    public let kind: Kind
    public let underlying: String?

    public init(kind: Kind, underlying: String? = nil) {
        self.kind = kind
        self.underlying = underlying
    }

    public static func == (lhs: SyncError, rhs: SyncError) -> Bool {
        lhs.kind == rhs.kind
    }
}

extension SyncError {
    /// Map a `URLError` to the closest `SyncError.Kind`. Spec §6.
    static func mapURLError(_ e: URLError) -> SyncError {
        switch e.code {
        case .cancelled:
            return SyncError(kind: .cancelled, underlying: e.localizedDescription)
        case .timedOut:
            return SyncError(kind: .connectTimeout, underlying: e.localizedDescription)
        case .notConnectedToInternet,
             .cannotFindHost,
             .cannotConnectToHost,
             .dnsLookupFailed,
             .networkConnectionLost,
             .resourceUnavailable,
             .secureConnectionFailed,
             .serverCertificateUntrusted,
             .serverCertificateHasBadDate,
             .serverCertificateHasUnknownRoot,
             .serverCertificateNotYetValid,
             .clientCertificateRejected,
             .clientCertificateRequired:
            return SyncError(kind: .networkUnreachable, underlying: e.localizedDescription)
        default:
            return SyncError(kind: .networkUnreachable, underlying: e.localizedDescription)
        }
    }

    /// Map an HTTP status code from a non-error response body.
    /// Returns nil for 2xx; throws for the rest. Spec §1, §6.
    static func mapHTTPStatus(_ status: Int) -> SyncError? {
        switch status {
        case 200, 201, 204:
            return nil
        case 401:
            return SyncError(kind: .authFailed)
        case 404:
            return SyncError(kind: .notFound)
        case 500...599:
            return SyncError(kind: .serverError(status))
        default:
            return SyncError(kind: .protocolError(status))
        }
    }
}
