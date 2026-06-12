import Foundation

/// Parses `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url>` URIs
/// minted by the desktop client when the user picks "Mobile sync" from its
/// pairing menu. The same URI travels three ways:
///
/// 1. Encoded as a QR code that the iOS Camera scans → opens via
///    `.onOpenURL` (custom URL scheme — requires `CFBundleURLTypes`).
/// 2. Encoded as a QR code that the in-app scanner reads → dispatched
///    through `ServerQRPayload.parse`.
/// 3. Pasted as a string into the manual-add form (future).
///
/// Spec: `docs/architecture/mobile-sync-connect-uri.md`. Identical Rust
/// and TypeScript parsers exist on the desktop side; the golden vector in
/// `ConnectURITests.parsesTheGoldenVector` is byte-equal across all three.
public enum ConnectURI {
    /// Decoded payload. `url` / `user` / `pwd` are spec-required; the
    /// `other` dict is forward-compatible — unknown keys land here and
    /// callers pick what they understand (today: `did`, `label`, `proto`).
    public struct Payload: Equatable, Hashable, Sendable, Identifiable {
        /// The canonical base URL. Equals `urls[0]`. Old clients read only
        /// this; new clients prefer `urls`.
        public let url: String
        /// §4 — ordered candidate base URLs for network-based switching.
        /// Always non-empty: when the QR omits `urls` (single-URL pairing,
        /// where the desktop skips the field), this is `[url]`.
        public let urls: [String]
        public let user: String
        public let pwd: String
        public let other: [String: String]

        public var deviceId: String? { other["did"] }
        public var label: String? { other["label"] }
        public var proto: String? { other["proto"] }

        /// SwiftUI `.sheet(item:)` needs `Identifiable`. The payload has
        /// no inherent identity beyond its fields; the URL is the
        /// stable-enough discriminator (two scans of the same QR are the
        /// same import).
        public var id: String { url }

        public init(url: String, urls: [String]? = nil, user: String, pwd: String, other: [String: String]) {
            self.url = url
            // Fall back to the single `url` when no candidate list is given,
            // so consumers can always read `urls` without a nil check.
            self.urls = (urls?.isEmpty == false) ? urls! : [url]
            self.user = user
            self.pwd = pwd
            self.other = other
        }
    }

    public enum ParseError: Error, Equatable, Sendable {
        /// Scheme isn't `uniclipboard://`. Probably scanned the wrong QR.
        case invalidScheme
        /// Protocol version (in the query or embedded in the payload) is
        /// something this app doesn't know how to read.
        case unsupportedVersion(found: Int)
        /// `svc` query parameter is something other than `mobile-sync`.
        case unsupportedService(found: String)
        /// `p` query parameter is missing, isn't base64url, or doesn't
        /// decode to a JSON object. `detail` is for logs, not UI copy.
        case payloadDecodeFailed(detail: String)
        /// A required key (`url`, `user`, `pwd`) is missing, null, or
        /// empty — the spec collapses all three into one error.
        case missingField(name: String)
        /// Payload's `url` is present but not parseable as http(s).
        case invalidURL(detail: String)
    }

    /// Parse a connect URI. The detail strings in `ParseError` are part of
    /// the cross-language contract — they're asserted by the golden test,
    /// so don't reword them without updating Rust + TS in lockstep.
    public static func parse(_ raw: String) throws -> Payload {
        // §4 step 1: split scheme. URLComponents preserves case on the
        // scheme but RFC 3986 says schemes are case-insensitive, so we
        // lowercase before comparing.
        guard let comp = URLComponents(string: raw),
              let scheme = comp.scheme?.lowercased(),
              scheme == "uniclipboard"
        else { throw ParseError.invalidScheme }

        // §4 step 2: read query params. Manually scan a small list — we
        // only need three, and `URLComponents.queryItems` handles the
        // percent-decoding for us.
        let items = comp.queryItems ?? []
        func q(_ name: String) -> String? {
            items.first(where: { $0.name == name })?.value
        }

        // §4 step 3: version. Required. Anything other than 1 means the
        // desktop is newer than this app — user needs to update.
        guard let vRaw = q("v"), let v = Int(vRaw) else {
            throw ParseError.unsupportedVersion(found: 0)
        }
        guard v == 1 else { throw ParseError.unsupportedVersion(found: v) }

        // §4 step 3b: service discriminator. Reserved for future variants
        // (e.g. `desktop-pair`, `web-pair`); only `mobile-sync` is wired
        // through this parser today.
        let svc = q("svc") ?? ""
        guard svc == "mobile-sync" else {
            throw ParseError.unsupportedService(found: svc)
        }

        // §4 step 4: base64url-no-pad → bytes
        guard let pParam = q("p") else {
            throw ParseError.payloadDecodeFailed(detail: "missing p")
        }
        guard let jsonBytes = base64URLDecode(pParam) else {
            throw ParseError.payloadDecodeFailed(detail: "invalid base64url")
        }

        // §4 step 5: bytes → JSON dict
        let rawJSON: Any
        do {
            rawJSON = try JSONSerialization.jsonObject(with: jsonBytes, options: [])
        } catch {
            throw ParseError.payloadDecodeFailed(detail: "\(error)")
        }
        guard let dict = rawJSON as? [String: Any] else {
            throw ParseError.payloadDecodeFailed(detail: "payload is not a JSON object")
        }

        // Embedded v must match the query v — extra defense against
        // hand-edited URIs.
        if let embeddedV = dict["v"] as? Int, embeddedV != 1 {
            throw ParseError.unsupportedVersion(found: embeddedV)
        }

        // §4 step 6: required field extraction. Empty == missing
        // (spec §4.2 collapses null / missing / empty into MISSING_FIELD).
        func requiredString(_ key: String) throws -> String {
            let s = (dict[key] as? String) ?? ""
            guard !s.isEmpty else { throw ParseError.missingField(name: key) }
            return s
        }

        let urlString = try requiredString("url")
        let user      = try requiredString("user")
        let pwd       = try requiredString("pwd")

        // §5 in the spec: URL must be http(s).
        guard isHTTPURL(urlString) else { throw ParseError.invalidURL(detail: urlString) }

        // §4: optional ordered candidate list. Each entry is a full base URL;
        // keep only well-formed http(s) strings (a hand-edited / future entry
        // we can't use is dropped, not fatal). An absent / all-dropped list
        // falls back to `[url]` inside `Payload.init`. We do NOT force
        // `urls[0] == url` — the desktop guarantees it; if a malformed QR
        // disagrees, the canonical `url` (already validated) still wins as the
        // first try via the init's fallback only when the list is empty.
        let urls: [String]? = (dict["urls"] as? [Any])?
            .compactMap { $0 as? String }
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { isHTTPURL($0) }

        // §3.2: forward-compatible — drop non-string `o.*` values silently.
        var other: [String: String] = [:]
        if let o = dict["o"] as? [String: Any] {
            for (k, v) in o {
                if let s = v as? String { other[k] = s }
            }
        }

        return Payload(url: urlString, urls: urls, user: user, pwd: pwd, other: other)
    }

    /// True iff `s` parses as an http(s) URL — the §5 scheme gate, shared by
    /// the canonical `url` and each `urls` candidate.
    private static func isHTTPURL(_ s: String) -> Bool {
        guard let parsed = URL(string: s),
              let scheme = parsed.scheme?.lowercased()
        else { return false }
        return scheme == "http" || scheme == "https"
    }

    /// base64url-no-pad → Data. Matches Rust `URL_SAFE_NO_PAD` and the
    /// TypeScript `btoa(...).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'')`.
    private static func base64URLDecode(_ s: String) -> Data? {
        var t = s.replacingOccurrences(of: "-", with: "+")
                 .replacingOccurrences(of: "_", with: "/")
        // Restore padding so Foundation accepts it.
        let pad = (4 - t.count % 4) % 4
        t += String(repeating: "=", count: pad)
        return Data(base64Encoded: t)
    }
}
