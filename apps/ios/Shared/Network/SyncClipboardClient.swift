import Foundation
import OSLog
#if canImport(UniClipboardModels)
// SwiftPM build: model types live in a separate target. The Xcode app
// target compiles everything as one module, so no import is needed —
// `canImport` is false there because the project doesn't depend on
// the local SwiftPM package.
import UniClipboardModels
#endif

private let log = Logger(subsystem: "app.uniclipboard", category: "network")

/// HTTP client for the SyncClipboard wire protocol.
/// Spec: docs/SYNC_PROTOCOL.md §1–§3 (read path only this cycle).
///
/// Not `@MainActor`-isolated so that callers on any actor can `await`
/// without an unnecessary hop. The Xcode app target's
/// `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor` is target-specific; the
/// SwiftPM `UniClipboardNetwork` target compiles this file without that
/// default, so don't decorate types here with `@MainActor`.
public final class SyncClipboardClient: @unchecked Sendable {
    private let baseURL: URL
    private let authHeader: String
    private let session: URLSession
    private let ownsSession: Bool

    /// Set by `cancelInFlight()`; checked at the top of `perform` so a
    /// cancelled client also refuses NEW requests (including the 300ms
    /// timedOut retry) instead of pointing them at a URL the caller just
    /// declared dead. Lock-guarded: callers cancel from the main actor
    /// while `perform` runs on a background executor.
    private let cancelLock = NSLock()
    private var isCancelled = false

    /// - Parameters:
    ///   - server: profile providing URL + credentials. URL is normalized
    ///     per §1.1 inside the init.
    ///   - trustInsecureCert: when true, the constructed URLSession uses
    ///     a delegate that accepts any server trust — for self-signed
    ///     LAN servers. Ignored when `session` is supplied (caller owns
    ///     trust policy in that case).
    ///   - session: optional pre-built session for tests. When supplied,
    ///     the client does not own its lifetime.
    public init(
        server: ServerConfig,
        trustInsecureCert: Bool,
        session: URLSession? = nil
    ) throws {
        self.baseURL = try Self.normalizeBaseURL(server.url)
        self.authHeader = Self.basicAuthHeader(username: server.username, password: server.password)
        if let session {
            self.session = session
            self.ownsSession = false
        } else {
            self.session = Self.makeSession(trustInsecureCert: trustInsecureCert)
            self.ownsSession = true
        }
    }

    deinit {
        if ownsSession { session.invalidateAndCancel() }
    }

    /// Abort every in-flight request on this client and poison it against
    /// new ones — all subsequent calls throw `SyncError(.cancelled)`.
    ///
    /// §5.3: called when the network path or the profile's live URL changes
    /// while a request is still talking to the now-dead URL. Without this,
    /// a GET against a blackholed route (e.g. a LAN IP reached over
    /// cellular) runs out the full request timeout. Cancels tasks rather
    /// than invalidating the session — `URLSession` raises an Objective-C
    /// exception if the retry path creates a task on an invalidated
    /// session; the `isCancelled` flag covers that window instead.
    public func cancelInFlight() {
        cancelLock.lock()
        isCancelled = true
        cancelLock.unlock()
        session.getAllTasks { tasks in
            for task in tasks { task.cancel() }
        }
    }

    private var wasCancelled: Bool {
        cancelLock.lock()
        defer { cancelLock.unlock() }
        return isCancelled
    }

    // MARK: - Endpoints

    /// `GET SyncClipboard.json` — pull current clipboard state. Spec §2.1.
    public func getClipboard() async throws -> Clipboard {
        let url = baseURL.appendingPathComponent("SyncClipboard.json")
        var req = URLRequest(url: url)
        req.httpMethod = "GET"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")

        let (data, response) = try await perform(req)
        try checkStatus(response, op: "getClipboard")
        do {
            return try JSONDecoder().decode(Clipboard.self, from: data)
        } catch {
            log.error("getClipboard: decode failed (\(data.count, privacy: .public) bytes): \(String(describing: error), privacy: .public)")
            throw SyncError(kind: .decodingFailed, underlying: "\(error)")
        }
    }

    /// `PUT SyncClipboard.json` — publish clipboard metadata. Spec §2.2.
    /// If `entry.hasData == true`, the payload file MUST already have been
    /// uploaded via `putFile(name:body:)` per §3.5 — this method does not
    /// enforce that itself; callers do.
    public func putClipboard(_ entry: Clipboard) async throws {
        let url = baseURL.appendingPathComponent("SyncClipboard.json")
        var req = URLRequest(url: url)
        req.httpMethod = "PUT"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        do {
            req.httpBody = try JSONEncoder().encode(entry)
        } catch {
            throw SyncError(kind: .decodingFailed, underlying: "\(error)")
        }
        let (_, response) = try await perform(req)
        try checkStatus(response, op: "putClipboard")
    }

    /// `GET file/<name>` — download payload bytes. Spec §2.4.
    /// Same filename guard as `putFile` — rejects `/`, `\`, empty before
    /// any network call. 404 surfaces as `.notFound` (the spec calls this
    /// a "server inconsistency" — metadata advertises `hasData=true` but
    /// the file is gone — but the existing not-found mapping is the right
    /// signal for callers).
    public func getFile(name: String) async throws -> Data {
        guard !name.isEmpty, !name.contains("/"), !name.contains("\\") else {
            throw SyncError(kind: .invalidURL, underlying: "invalid filename: \(name)")
        }
        let url = baseURL
            .appendingPathComponent("file")
            .appendingPathComponent(name)
        var req = URLRequest(url: url)
        req.httpMethod = "GET"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")
        let (data, response) = try await perform(req)
        try checkStatus(response, op: "getFile")
        return data
    }

    /// `GET /api/history/<profileId>/data` — download a history record's
    /// payload bytes. Spec §2.11.
    ///
    /// `profileId` is the composite `<type>-<hash>` form (same as §2.8,
    /// **not** the split form used by §2.10 PATCH). Callers either
    /// construct it via `HistoryRecord.profileId(type:hash:)` or read
    /// it off `HistoryRecord.id` directly.
    ///
    /// 404 surfaces as `.notFound` — for §2.11 that means either the
    /// record never existed or it was soft-deleted with bytes garbage-
    /// collected. Callers treat both as "absent".
    public func getHistoryPayload(profileId: String) async throws -> Data {
        guard !profileId.isEmpty, !profileId.contains("/"), !profileId.contains("\\") else {
            throw SyncError(kind: .invalidURL, underlying: "invalid profileId: \(profileId)")
        }
        let url = baseURL
            .appendingPathComponent("api")
            .appendingPathComponent("history")
            .appendingPathComponent(profileId)
            .appendingPathComponent("data")
        var req = URLRequest(url: url)
        req.httpMethod = "GET"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")
        let (data, response) = try await perform(req)
        try checkStatus(response, op: "getHistoryPayload")
        return data
    }

    /// `POST /api/history/query` — paginated history listing. Spec §2.7.
    ///
    /// Filters are sent as `multipart/form-data` per the wire contract.
    /// Returns `[HistoryRecord]` (possibly empty — an empty page is the
    /// documented end-of-list signal, NOT an error).
    ///
    /// Pagination: callers loop with `page = 1, 2, …` until they receive
    /// an empty array. Incremental sync: pass the highest `lastModified`
    /// seen so far as `modifiedAfter` to fetch only the delta. The
    /// server keys off `lastModified > modifiedAfter` (strict inequality).
    public func queryHistory(_ query: HistoryQuery = HistoryQuery()) async throws -> [HistoryRecord] {
        let url = baseURL
            .appendingPathComponent("api")
            .appendingPathComponent("history")
            .appendingPathComponent("query")
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")

        let body = query.multipartEncoded()
        req.setValue(body.contentType, forHTTPHeaderField: "Content-Type")
        req.httpBody = body.encoded()

        let (data, response) = try await perform(req)
        try checkStatus(response, op: "queryHistory")
        do {
            return try JSONDecoder().decode([HistoryRecord].self, from: data)
        } catch {
            log.error("queryHistory: decode failed (\(data.count, privacy: .public) bytes): \(String(describing: error), privacy: .public)")
            throw SyncError(kind: .decodingFailed, underlying: "\(error)")
        }
    }

    /// `PUT file/<name>` — upload payload file. Spec §2.3.
    /// Rejects names containing `/`, `\`, or empty before any network
    /// call; spec mandates "MUST NOT contain path separators".
    public func putFile(name: String, body: Data) async throws {
        guard !name.isEmpty, !name.contains("/"), !name.contains("\\") else {
            throw SyncError(kind: .invalidURL, underlying: "invalid filename: \(name)")
        }
        let url = baseURL
            .appendingPathComponent("file")
            .appendingPathComponent(name)
        var req = URLRequest(url: url)
        req.httpMethod = "PUT"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")
        req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
        req.setValue("\(body.count)", forHTTPHeaderField: "Content-Length")
        req.httpBody = body
        let (_, response) = try await perform(req)
        try checkStatus(response, op: "putFile")
    }

    // MARK: - Internals

    /// Map non-2xx statuses to `SyncError` and log them — HTTP-level
    /// failures (401/404/5xx) previously threw without a trace, leaving
    /// only the caller's aggregated state to debug from.
    private func checkStatus(_ response: URLResponse, op: StaticString) throws {
        let status = (response as? HTTPURLResponse)?.statusCode ?? -1
        guard let err = SyncError.mapHTTPStatus(status) else { return }
        // 404 on GETs is the documented "empty server" state — routine,
        // not error-worthy.
        if err.kind == .notFound {
            log.debug("\(op, privacy: .public): HTTP \(status, privacy: .public) (not found)")
        } else {
            log.error("\(op, privacy: .public): HTTP \(status, privacy: .public) → \(String(describing: err.kind), privacy: .public)")
        }
        throw err
    }

    private func perform(_ req: URLRequest, attempt: Int = 1) async throws -> (Data, URLResponse) {
        if wasCancelled {
            throw SyncError(kind: .cancelled, underlying: "client cancelled via cancelInFlight()")
        }
        do {
            return try await session.data(for: req)
        } catch let e as URLError {
            // -1005 networkConnectionLost / -1001 timedOut: iOS 进程内
            // NWConnection / NECP 路径会僵在 dead state — 即使新建
            // ephemeral URLSession 也会复用上一段坏 path，连续命中同样
            // 的错误，必须重启 app 才恢复。Apple 在 forums/thread/660771
            // 给的官方 workaround 是延迟 ~300ms 重试一次：二次请求会
            // 重新走 path 评估，多数情况下能拿到 fresh connection。
            let retriable: Set<URLError.Code> = [.networkConnectionLost, .timedOut]
            if attempt == 1, retriable.contains(e.code) {
                log.warning("perform: URLError \(e.code.rawValue, privacy: .public) on \(req.httpMethod ?? "?", privacy: .public) \(req.url?.absoluteString ?? "?", privacy: .public) — retrying once after 300ms")
                try? await Task.sleep(nanoseconds: 300_000_000)
                return try await perform(req, attempt: 2)
            }
            log.error("perform: URLError \(e.code.rawValue, privacy: .public) on \(req.httpMethod ?? "?", privacy: .public) \(req.url?.absoluteString ?? "?", privacy: .public) attempt=\(attempt, privacy: .public): \(e.localizedDescription, privacy: .public)")
            throw SyncError.mapURLError(e)
        } catch {
            log.error("perform: non-URLError on \(req.httpMethod ?? "?", privacy: .public) \(req.url?.absoluteString ?? "?", privacy: .public): \(String(describing: error), privacy: .public)")
            throw SyncError(kind: .networkUnreachable, underlying: "\(error)")
        }
    }

    // MARK: - Helpers (testable as static)

    /// §1.1 — trim whitespace, append trailing slash if missing, validate
    /// scheme is http or https.
    static func normalizeBaseURL(_ raw: String) throws -> URL {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw SyncError(kind: .invalidURL) }
        let withSlash = trimmed.hasSuffix("/") ? trimmed : trimmed + "/"
        guard let url = URL(string: withSlash) else { throw SyncError(kind: .invalidURL) }
        guard let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              url.host?.isEmpty == false
        else {
            throw SyncError(kind: .invalidURL)
        }
        return url
    }

    /// §1.2 — `Basic <base64(utf8(user:pass))>`.
    static func basicAuthHeader(username: String, password: String) -> String {
        let pair = "\(username):\(password)"
        let encoded = Data(pair.utf8).base64EncodedString()
        return "Basic \(encoded)"
    }

    private static func makeSession(trustInsecureCert: Bool) -> URLSession {
        let cfg = URLSessionConfiguration.ephemeral
        // §1 timeouts. `timeoutIntervalForRequest` is an IDLE timer (resets
        // whenever data arrives), so 10s doesn't cap large transfers — the
        // 5min resource clock does. 10s instead of the spec's nominal 5s
        // connect: URLSession has no separate connect timeout, and the idle
        // timer also spans server think-time on slow LAN boxes. It's the
        // backstop for a blackholed route (LAN IP over cellular) when no
        // path-change event fired to cancel the request explicitly.
        cfg.timeoutIntervalForRequest = 10
        cfg.timeoutIntervalForResource = 5 * 60
        if trustInsecureCert {
            let delegate = TrustingDelegate()
            return URLSession(configuration: cfg, delegate: delegate, delegateQueue: nil)
        }
        return URLSession(configuration: cfg)
    }
}

/// Accepts any server trust — used only when the user opts into
/// "trust insecure cert" for LAN/self-signed servers (§1). Internal (not
/// private) so `ConnectionTester.probe`'s short-timeout session can carry
/// the same trust policy.
final class TrustingDelegate: NSObject, URLSessionDelegate, Sendable {
    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @Sendable @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        if challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
           let trust = challenge.protectionSpace.serverTrust {
            completionHandler(.useCredential, URLCredential(trust: trust))
        } else {
            completionHandler(.performDefaultHandling, nil)
        }
    }
}
