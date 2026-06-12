import Foundation

/// §5.1 — coarse classification of a base URL by the kind of network path it
/// reaches, derived purely from the host (no DNS, no probing). Drives the
/// network-based URL ordering in §5.3: a config now carries several candidate
/// URLs and the effective one is picked by reachability over a shape-ordered
/// list. Pure `String` math, so it stays valid in the UIKit/SwiftUI-free
/// `Shared/` layer.
public enum ServerURLClass: String, Sendable, Equatable {
    case lan        // RFC1918 / link-local IPv4, or *.local mDNS host
    case tailscale  // Tailscale CGNAT 100.64.0.0/10, or *.ts.net MagicDNS host
    case wan        // everything else (public IP or public hostname)
}

/// One server profile. Spec: docs/SYNC_PROTOCOL.md §5.1.
///
/// A profile is one logical server identity (one credential pair, one device
/// id from the pairing QR) reachable at one or more candidate base URLs
/// (`urls`). The candidate list is ordered by the publisher but re-ordered at
/// runtime by the device's current network (§5.3) so the client prefers a
/// LAN/Tailscale direct path when it's up and falls back to the public URL
/// otherwise. There is no per-profile network strategy anymore — auto-switch
/// happens *within* a profile, between its URLs.
public struct ServerConfig: Codable, Equatable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String?
    /// Ordered candidate base URLs, each a complete base URL (the parser/UI
    /// trim trailing slashes, but consumers must tolerate one). Never empty
    /// for a valid config — a decode that finds neither `urls` nor the legacy
    /// `url` throws. `urls[0]` is the canonical default and equals the legacy
    /// `url` field old clients read.
    public var urls: [String]
    public var username: String
    public var password: String

    /// Back-compat accessor == `urls[0]`. Most call sites (the client
    /// builder, the connection tester) take a single URL; they read this.
    /// Empty string only when `urls` is somehow empty — the list layer
    /// refuses to make calls in that case (mirrors `activeConfig == nil`).
    public var url: String { urls.first ?? "" }

    public init(
        id: String,
        name: String? = nil,
        urls: [String],
        username: String,
        password: String
    ) {
        self.id = id
        self.name = name
        self.urls = urls
        self.username = username
        self.password = password
    }

    /// Single-URL convenience — the common case for hand-built configs,
    /// legacy migration, and the connection-test probe. Wraps `url` into a
    /// one-element `urls`.
    public init(
        id: String,
        name: String? = nil,
        url: String,
        username: String,
        password: String
    ) {
        self.init(id: id, name: name, urls: [url], username: username, password: password)
    }

    private enum CodingKeys: String, CodingKey {
        case id, name, url, urls, username, password
        // Decoded-and-dropped: the pre-multi-URL model persisted a per-config
        // auto-switch strategy + SSID list here. Auto-switch is now between a
        // profile's URLs, not between profiles, so these keys are ignored on
        // read and never re-encoded.
        case autoSwitchWifiNames, autoSwitchStrategy
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id       = try c.decode(String.self, forKey: .id)
        name     = try c.decodeIfPresent(String.self, forKey: .name)
        username = try c.decode(String.self, forKey: .username)
        password = try c.decode(String.self, forKey: .password)
        // `urls` is the source of truth when present and non-empty. Older data
        // (and the wire payload's `skip_serializing_if = Vec::is_empty` case)
        // omits it, so fall back to the legacy single `url` (== urls[0]). At
        // least one of the two must be present.
        let urlsField = try c.decodeIfPresent([String].self, forKey: .urls)
        let urlField  = try c.decodeIfPresent(String.self, forKey: .url)
        if let urlsField, !urlsField.isEmpty {
            urls = urlsField
        } else if let urlField {
            urls = [urlField]
        } else {
            throw DecodingError.dataCorrupted(
                DecodingError.Context(
                    codingPath: c.codingPath,
                    debugDescription: "ServerConfig requires non-empty `urls` or a legacy `url`"
                )
            )
        }
        // `autoSwitchWifiNames` / `autoSwitchStrategy` intentionally ignored.
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encodeIfPresent(name, forKey: .name)
        // Emit BOTH keys: `url` (== urls[0]) so an old single-URL reader still
        // works, and `urls` for the full candidate list. Mirrors the §4 wire
        // payload, where `url` and `urls[0]` are kept identical on purpose.
        try c.encode(url, forKey: .url)
        try c.encode(urls, forKey: .urls)
        try c.encode(username, forKey: .username)
        try c.encode(password, forKey: .password)
    }

    /// §5.1 — fall back to the canonical URL when name is nil/empty/whitespace.
    public var displayLabel: String {
        if let n = name?.trimmingCharacters(in: .whitespacesAndNewlines), !n.isEmpty { return n }
        return url
    }

    /// §5.1 SSID normalization: trim → strip outer quotes → reject Android
    /// privacy placeholders. SSID names are no longer matched for auto-switch
    /// (that's URL-based now), but the value still flows cross-process as a
    /// "which Wi-Fi am I on" signal (App Group `last_known_ssid`), so the
    /// normalization rule stays the single source of truth.
    public static func normalizeSSID(_ raw: String?) -> String? {
        guard let raw else { return nil }
        var s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.count >= 2, s.hasPrefix("\""), s.hasSuffix("\"") {
            s = String(s.dropFirst().dropLast())
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }
        if s.isEmpty || s == "<unknown ssid>" || s == "0x" { return nil }
        return s
    }

    // MARK: - §5.3 URL classification + network ordering

    /// Classify a base URL by the network path it most likely reaches, from
    /// the host alone (no DNS resolution, no probing). Hostname heuristics
    /// win over numeric parsing: `*.ts.net` (MagicDNS) → tailscale, `*.local`
    /// (mDNS) → lan. Numeric IPv4 hosts use the standard private / CGNAT
    /// ranges. Anything else — a public IP or any other hostname — is `wan`.
    public static func classifyURL(_ urlString: String) -> ServerURLClass {
        guard let host = URL(string: urlString.trimmingCharacters(in: .whitespacesAndNewlines))?
            .host?
            .lowercased(),
              !host.isEmpty
        else { return .wan }
        if host.hasSuffix(".ts.net") { return .tailscale }
        if host.hasSuffix(".local")  { return .lan }
        if let ipClass = classifyIPv4(host) { return ipClass }
        return .wan
    }

    /// Returns the class for a dotted-quad IPv4 host, or nil when `host` is
    /// not a numeric IPv4 literal (so the caller treats it as a hostname).
    private static func classifyIPv4(_ host: String) -> ServerURLClass? {
        let parts = host.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else { return nil }
        var octets: [Int] = []
        octets.reserveCapacity(4)
        for part in parts {
            guard let v = Int(part), (0...255).contains(v) else { return nil }
            octets.append(v)
        }
        let a = octets[0], b = octets[1]
        if a == 100, (64...127).contains(b) { return .tailscale }   // 100.64.0.0/10
        if a == 10                          { return .lan }         // 10.0.0.0/8
        if a == 172, (16...31).contains(b)  { return .lan }         // 172.16.0.0/12
        if a == 192, b == 168               { return .lan }         // 192.168.0.0/16
        if a == 169, b == 254               { return .lan }         // 169.254.0.0/16
        return .wan
    }

    /// §5.3 — the preferred URL-class order for `network`, or nil when the
    /// network gives no useful signal (keep the publisher's order).
    ///
    /// On Wi-Fi the direct LAN path is lowest-latency, so it leads **even when
    /// Tailscale is also up** — reachability probing demotes the LAN URL on a
    /// foreign network where it can't connect, so ranking it first is safe.
    /// Tailscale is the next-best fallback (a direct/encrypted path to the same
    /// server), then the public WAN relay. Off Wi-Fi with Tailscale up (e.g.
    /// cellular + Tailscale) the Tailscale URL leads; plain cellular prefers the
    /// WAN relay and de-prioritizes the LAN URL so we don't waste a probe
    /// timeout on a path that can't work.
    static func classPreference(_ network: NetworkContext) -> [ServerURLClass]? {
        // SSID name is no longer matched, but a non-nil SSID still means "on a
        // named Wi-Fi" — use it as a fallback wifi signal for clients that
        // don't populate `isWifi` yet.
        let onWifi = network.isWifi || network.ssid != nil
        if onWifi              { return [.lan, .tailscale, .wan] }
        if network.isTailscale { return [.tailscale, .wan, .lan] }
        if network.isCellular  { return [.wan, .tailscale, .lan] }
        return nil
    }

    /// §5.3 — this config's candidate URLs re-ordered for `network`. A stable
    /// sort: URLs of a more-preferred class move ahead, but within one class
    /// (and when the network gives no signal) the publisher's original order
    /// is preserved. Reachability is NOT consulted here — this only decides
    /// the *try order*; the app layer probes the result and the keyboard
    /// extension uses `[0]` as its best guess.
    public func orderedURLs(network: NetworkContext) -> [String] {
        guard let preference = Self.classPreference(network) else { return urls }
        func rank(_ u: String) -> Int {
            preference.firstIndex(of: Self.classifyURL(u)) ?? preference.count
        }
        return urls.enumerated()
            .sorted { lhs, rhs in
                let lr = rank(lhs.element), rr = rank(rhs.element)
                return lr != rr ? lr < rr : lhs.offset < rhs.offset
            }
            .map(\.element)
    }

    /// §5.3 — the try-order with the probe's verdict layered on top: a
    /// `live` URL the app's last probe confirmed reachable leads, the
    /// remaining candidates follow in shape order as fallbacks. A `live`
    /// value not in `urls` (the config was edited since the probe wrote
    /// it) is ignored rather than resurrected. Pure — `live` comes from
    /// `SettingsStore.loadLiveURL`, `network` from the caller's monitor.
    public func preferredURLs(live: String?, network: NetworkContext) -> [String] {
        let ordered = orderedURLs(network: network)
        guard let live, urls.contains(live) else { return ordered }
        return [live] + ordered.filter { $0 != live }
    }
}

/// Persisted multi-server collection. Spec: §5.2.
public struct ServerConfigList: Codable, Equatable, Hashable, Sendable {
    public var configs: [ServerConfig]
    public var activeConfigId: String?

    public init(
        configs: [ServerConfig] = [],
        activeConfigId: String? = nil
    ) {
        self.configs = configs
        self.activeConfigId = activeConfigId
    }

    private enum CodingKeys: String, CodingKey {
        // `manualOverrideConfigId` is decode-only: a pre-unification key we
        // migrate away from (see init(from:)) and never re-encode.
        case configs, activeConfigId, manualOverrideConfigId
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        configs = try c.decodeIfPresent([ServerConfig].self, forKey: .configs) ?? []
        let decodedActive = try c.decodeIfPresent(String.self, forKey: .activeConfigId)
        // One-shot migration: pre-unification builds persisted a home-chip
        // "pin" in `manualOverrideConfigId` that out-prioritized
        // `activeConfigId`. The pin concept is gone — the user's last
        // explicit pick IS the current server now — so promote a resolvable
        // legacy pin into `activeConfigId` and never re-encode the old key
        // (see encode(to:)). Absent/unresolvable → fall back to the
        // persisted `activeConfigId`.
        let legacyPin = try c.decodeIfPresent(String.self, forKey: .manualOverrideConfigId)
        if let pin = legacyPin, configs.contains(where: { $0.id == pin }) {
            activeConfigId = pin
        } else {
            activeConfigId = decodedActive
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(configs, forKey: .configs)
        try c.encodeIfPresent(activeConfigId, forKey: .activeConfigId)
    }

    /// §5.2 — stale activeConfigId falls back to configs[0]; nil iff configs is empty.
    public var activeConfig: ServerConfig? {
        guard !configs.isEmpty else { return nil }
        if let id = activeConfigId, let hit = configs.first(where: { $0.id == id }) { return hit }
        return configs.first
    }

    /// §5.3 — the server to use *right now*: the manual baseline (§5.2
    /// `activeConfig`) with its candidate URLs re-ordered for the current
    /// network so `urls[0]` is the best path to try first. `nil` only when
    /// there's no config at all (mirrors `activeConfig`).
    ///
    /// Pure read — never mutates `activeConfigId` or the persisted candidate
    /// order. Which *profile* is active is still the user's manual pick (§5.2);
    /// the network only re-orders that profile's URLs. The main app and the
    /// keyboard extension both call this so they agree on the try-order; the
    /// app then confirms reachability by probing (§5.3 runtime).
    public func effectiveActiveConfig(network: NetworkContext) -> ServerConfig? {
        guard let base = activeConfig else { return nil }
        var cfg = base
        cfg.urls = base.orderedURLs(network: network)
        return cfg
    }
}

/// Read-only legacy single-config shape. Spec: §5.5.
public struct LegacyServerConfig: Codable, Equatable, Sendable {
    public var url: String
    public var username: String
    public var password: String

    public init(url: String, username: String, password: String) {
        self.url = url
        self.username = username
        self.password = password
    }

    /// §5.5 — wrap into a ServerConfigList with a fresh UUID v4 and mark active.
    /// The single legacy `url` becomes the one-element `urls` candidate list.
    public func migrated(idProvider: () -> String = { UUID().uuidString.lowercased() }) -> ServerConfigList {
        let cfg = ServerConfig(
            id: idProvider(),
            name: nil,
            urls: [url],
            username: username,
            password: password
        )
        return ServerConfigList(configs: [cfg], activeConfigId: cfg.id)
    }
}
