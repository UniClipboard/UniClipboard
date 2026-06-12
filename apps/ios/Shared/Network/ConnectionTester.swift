import Foundation
#if canImport(UniClipboardModels)
// SwiftPM build: model types live in a separate target. The Xcode app
// target compiles everything as one module, so no import is needed.
import UniClipboardModels
#endif

/// Probes a server's reachability + credentials via `getClipboard`.
/// Shared by SetupFlow's first-run form and Settings' add/edit forms so the
/// "测试连接" semantics are identical everywhere.
///
/// Spec §2.1 treats 404 as "no clipboard published yet" — the server is
/// reachable and auth is fine, which is what the user is testing. We map
/// that case to `.success`.
enum ConnectionTester {
    enum Result: Equatable, Sendable {
        case success
        case authFailed
        case unreachable
        case missingFields

        /// §5.3 probe semantics: a URL is *reachable* when the server
        /// answered at all — `.success` (200/404) or `.authFailed` (401).
        /// Bad credentials are an account problem, not a path problem;
        /// the URL picker must not skip a perfectly good direct path just
        /// because the password is stale (the engine surfaces authFailed
        /// on its own).
        var isReachable: Bool {
            self == .success || self == .authFailed
        }
    }

    static func test(
        url: String,
        username: String,
        password: String,
        trustInsecureCert: Bool
    ) async -> Result {
        let trimmedURL = url.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedURL.isEmpty || username.isEmpty || password.isEmpty {
            return .missingFields
        }
        let probe = ServerConfig(
            id: "probe",
            url: trimmedURL,
            username: username,
            password: password
        )
        let client: SyncClipboardClient
        do {
            client = try SyncClipboardClient(server: probe, trustInsecureCert: trustInsecureCert)
        } catch {
            return .unreachable
        }
        do {
            _ = try await client.getClipboard()
            return .success
        } catch let e as SyncError {
            switch e.kind {
            case .notFound:    return .success
            case .authFailed:  return .authFailed
            default:           return .unreachable
            }
        } catch {
            return .unreachable
        }
    }

    // MARK: - §5.3 multi-URL reachability probe

    /// Probe every candidate URL of a profile concurrently and report
    /// per-URL reachability. Used by the app's live-endpoint refresh (pick
    /// the first reachable URL in §5.3 shape order) and by the "测试连接"
    /// UI (show ✓/✗ per candidate).
    ///
    /// Differences from `test(url:…)`, deliberately:
    /// - Short timeout (default 2s vs the client's 30s) — a probe answers
    ///   "is this path up *right now*", and candidates that can't work on
    ///   the current network (e.g. a LAN IP on cellular) must fail fast.
    /// - No retry, no body decode — `GET SyncClipboard.json`'s status code
    ///   alone carries the reachability signal (§2.1: 404 = reachable but
    ///   empty; 401 = reachable, credentials wrong).
    ///
    /// `nonisolated` so the concurrent waits never funnel through the main
    /// actor (the Xcode targets compile `Shared/` with
    /// `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor`).
    ///
    /// - Parameters:
    ///   - session: test injection seam. When supplied, the caller owns its
    ///     lifetime AND its timeout/trust policy; `timeout` and
    ///     `trustInsecureCert` are ignored.
    /// - Returns: one `Result` per *distinct* URL string in `urls`.
    nonisolated static func probe(
        urls: [String],
        username: String,
        password: String,
        trustInsecureCert: Bool,
        timeout: TimeInterval = 2.0,
        session: URLSession? = nil
    ) async -> [String: Result] {
        let distinct = Array(Set(urls))
        guard !distinct.isEmpty else { return [:] }
        if username.isEmpty || password.isEmpty {
            return Dictionary(uniqueKeysWithValues: distinct.map { ($0, Result.missingFields) })
        }
        let ownsSession = session == nil
        let probeSession = session ?? Self.makeProbeSession(
            trustInsecureCert: trustInsecureCert,
            timeout: timeout
        )
        defer { if ownsSession { probeSession.invalidateAndCancel() } }
        let authHeader = SyncClipboardClient.basicAuthHeader(username: username, password: password)
        var results: [String: Result] = [:]
        results.reserveCapacity(distinct.count)
        await withTaskGroup(of: (String, Result).self) { group in
            for url in distinct {
                group.addTask {
                    (url, await Self.probeOne(url: url, authHeader: authHeader, session: probeSession))
                }
            }
            for await (url, result) in group {
                results[url] = result
            }
        }
        return results
    }

    /// The §5.3 pick: the first URL in `orderedURLs` (shape order for the
    /// current network) whose probe came back reachable, or nil when no
    /// candidate is. Pure — deterministic given the probe results, NOT a
    /// race; two reachable candidates resolve to whichever ranks earlier.
    nonisolated static func firstReachable(
        in orderedURLs: [String],
        results: [String: Result]
    ) -> String? {
        orderedURLs.first { results[$0]?.isReachable == true }
    }

    /// One candidate: `GET <base>/SyncClipboard.json`, status-only.
    private nonisolated static func probeOne(
        url: String,
        authHeader: String,
        session: URLSession
    ) async -> Result {
        let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return .missingFields }
        guard let base = try? SyncClipboardClient.normalizeBaseURL(trimmed) else {
            return .unreachable
        }
        var req = URLRequest(url: base.appendingPathComponent("SyncClipboard.json"))
        req.httpMethod = "GET"
        req.setValue(authHeader, forHTTPHeaderField: "Authorization")
        do {
            let (_, response) = try await session.data(for: req)
            guard let status = (response as? HTTPURLResponse)?.statusCode else {
                return .unreachable
            }
            guard let err = SyncError.mapHTTPStatus(status) else { return .success }
            switch err.kind {
            case .notFound:   return .success
            case .authFailed: return .authFailed
            default:          return .unreachable
            }
        } catch {
            return .unreachable
        }
    }

    /// Ephemeral session with the probe's short timeout on both the
    /// request and resource clocks, carrying the same trust-insecure
    /// policy as the real client.
    private nonisolated static func makeProbeSession(
        trustInsecureCert: Bool,
        timeout: TimeInterval
    ) -> URLSession {
        let cfg = URLSessionConfiguration.ephemeral
        cfg.timeoutIntervalForRequest = timeout
        cfg.timeoutIntervalForResource = timeout
        // Never wait for connectivity: "no route right now" IS the answer.
        cfg.waitsForConnectivity = false
        if trustInsecureCert {
            return URLSession(configuration: cfg, delegate: TrustingDelegate(), delegateQueue: nil)
        }
        return URLSession(configuration: cfg)
    }
}
