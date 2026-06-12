//! Network-shape classification and multi-URL try-order resolution
//! (spec §5.1–§5.3; regression checklist B).
//!
//! Byte-for-byte port of the pure logic in the iOS app's
//! `Shared/Models/ServerConfig.swift` (`classifyURL`, `normalizeSSID`,
//! `classPreference`, `orderedURLs`, `preferredURLs`, `activeConfig`) and the
//! `NetworkContext` input shape from `Shared/Models/NetworkContext.swift`.
//! The Swift implementation and its tests
//! (`Tests/UniClipboardModelsTests/FixturesTests.swift`) are NORMATIVE — any
//! behavioral change here must first land on the Swift side.
//!
//! Everything in this module is a pure function over plain inputs: no DNS, no
//! reachability probing, no platform APIs, no I/O. Reachability probing and
//! `live`-URL persistence belong to the app layer.
//!
//! ## BYTE-CRITICAL invariants (the cross-platform contract)
//! - LAN IPv4 ranges: 10.0.0.0/8, 172.16.0.0/12 (172.16–172.31 inclusive,
//!   172.32 is WAN), 192.168.0.0/16, 169.254.0.0/16.
//! - Tailscale CGNAT: 100.64.0.0/10 (second octet 64..=127, i.e. the range
//!   ends at 100.127.255.255; 100.128.x is WAN).
//! - Hostname suffixes: `*.ts.net` → Tailscale, `*.local` → LAN. The suffix
//!   match includes the leading dot, so `fakets.net` must NOT match `.ts.net`.
//! - Hostname suffix checks run BEFORE the IPv4 literal check (Swift order).
//! - Ports and userinfo are ignored; the host alone decides the class.
//! - Ordering is a STABLE sort: within one class the publisher's original
//!   order is preserved; "no network signal" keeps the original order whole.

// ─── public types ───────────────────────────────────────────────────────

/// §5.1 — coarse classification of a base URL by the kind of network path it
/// reaches, derived purely from the host. Mirrors `ServerURLClass` in
/// `ServerConfig.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerUrlClass {
    /// RFC 1918 / link-local IPv4, or `*.local` mDNS host.
    Lan,
    /// Tailscale CGNAT 100.64.0.0/10, or `*.ts.net` MagicDNS host.
    Tailscale,
    /// Everything else (public IP or public hostname).
    Wan,
}

/// A snapshot of the device's current network, fed to the §5.3 auto-switch
/// resolver. Mirrors `NetworkContext` in `NetworkContext.swift`, minus the
/// platform detectors (`NWPathMonitor` / `getifaddrs`) — callers supply
/// plain booleans gathered by whatever platform layer they have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkContext {
    /// Normalized §5.1 SSID (see [`normalize_ssid`]), or `None` when not on
    /// a named Wi-Fi. A `Some` SSID acts as a fallback "on Wi-Fi" signal for
    /// callers that do not populate `is_wifi` (`NetworkContext.swift` docs).
    pub ssid: Option<String>,
    /// The primary path uses Wi-Fi.
    pub is_wifi: bool,
    /// The primary path is cellular data.
    pub is_cellular: bool,
    /// A Tailscale virtual network is up (an interface holds a 100.64.0.0/10
    /// IPv4). Highest-priority tier off Wi-Fi: it overlays the physical link.
    pub is_tailscale: bool,
}

// ─── §5.1 SSID normalization ────────────────────────────────────────────

/// §5.1 SSID normalization (`ServerConfig.normalizeSSID` in
/// `ServerConfig.swift`): trim → strip EXACTLY ONE layer of surrounding
/// double quotes (then trim again) → reject Android privacy placeholders
/// (`<unknown ssid>`, `0x`) and the empty string by returning `None`.
///
/// BYTE-CRITICAL: only one quote layer is stripped — `""Home""` normalizes
/// to `"Home"` (inner quotes kept), exactly like the Swift source. The
/// placeholder check runs AFTER quote stripping, so a quoted placeholder
/// also maps to `None`.
pub fn normalize_ssid(raw: Option<&str>) -> Option<String> {
    let mut s = raw?.trim();
    // Swift: `s.count >= 2, s.hasPrefix("\""), s.hasSuffix("\"")`. The quote
    // is ASCII, so the byte slicing below stays on char boundaries.
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s = s[1..s.len() - 1].trim();
    }
    if s.is_empty() || s == "<unknown ssid>" || s == "0x" {
        return None;
    }
    Some(s.to_string())
}

// ─── §5.1 URL classification (host shape) ───────────────────────────────

/// §5.1 — classify a base URL by the network path it most likely reaches,
/// from the host alone (no DNS resolution, no probing). Port of
/// `ServerConfig.classifyURL` in `ServerConfig.swift`.
///
/// Hostname heuristics win over numeric parsing: `*.ts.net` (MagicDNS) →
/// Tailscale, `*.local` (mDNS) → LAN, in that order, BEFORE the IPv4
/// literal check. Numeric IPv4 hosts use the standard private / CGNAT
/// ranges. Anything else — a public IP, any other hostname, or an
/// unparsable URL — is [`ServerUrlClass::Wan`].
pub fn classify_url(url_string: &str) -> ServerUrlClass {
    let Some(host) = extract_host(url_string.trim()) else {
        // Swift: `URL(string:)?.host` nil or empty → .wan
        return ServerUrlClass::Wan;
    };
    // BYTE-CRITICAL: suffix matching includes the leading dot —
    // "fakets.net" does NOT end with ".ts.net" and must stay WAN.
    if host.ends_with(".ts.net") {
        return ServerUrlClass::Tailscale;
    }
    if host.ends_with(".local") {
        return ServerUrlClass::Lan;
    }
    if let Some(ip_class) = classify_ipv4(&host) {
        return ip_class;
    }
    ServerUrlClass::Wan
}

/// Extracts the (lowercased) host from a URL string, mirroring what Swift's
/// `URL(string:)?.host?.lowercased()` yields for the inputs this module
/// cares about. Hand-rolled instead of the `url` crate because WHATWG
/// parsing canonicalizes exotic IPv4 literals (octal/hex octets), while
/// Swift's RFC 3986 `.host` returns the literal text — and the literal text
/// is what `classifyIPv4` (and therefore the cross-platform contract) sees.
///
/// Returns `None` when there is no authority component (no `://`), the
/// scheme is not RFC 3986-valid, the port is non-numeric, the host contains
/// a character Foundation rejects (space, `\`, `^`, `|`, …) or a malformed
/// percent escape, or the host is empty — all cases where Swift ends up
/// with a nil/empty host and `classifyURL` falls back to WAN.
///
/// Foundation's `.host` returns the PERCENT-DECODED text (verified against
/// Swift 6.2 Foundation: `http://x.ts%2Enet` → host `x.ts.net` → Tailscale,
/// `http://%31%30.0.0.5` → host `10.0.0.5` → LAN), so this mirror decodes
/// too. Known residual divergence: Foundation IDNA-encodes non-ASCII labels
/// (`café.local` → `xn--caf-dma.local`), which never changes the class for
/// ASCII suffixes, EXCEPT the pathological IDNA dot-mapping case
/// (`10。0。0。5` with fullwidth dots → Swift LAN, here WAN) — accepted as
/// out of scope for server-config base URLs.
fn extract_host(s: &str) -> Option<String> {
    let (scheme, rest) = s.split_once("://")?;
    // RFC 3986 scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ); a bogus
    // scheme makes Swift's `URL(string:)` return nil → host nil → WAN.
    let mut scheme_chars = scheme.chars();
    let valid_scheme = scheme_chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme {
        return None;
    }
    // The authority ends at the first '/', '?' or '#'.
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    // Drop userinfo (everything up to the last '@'). Foundation is lenient
    // here — invalid characters or escapes BEFORE the '@' never invalidate
    // the host (verified: `http://a%zz@nas.local` → host `nas.local`).
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        // Bracketed host — Foundation's `.host` strips the brackets and is
        // lenient about the contents (verified: `http://[10.0.0.5]` → host
        // `10.0.0.5` → LAN, `http://[x.local]` → `x.local`), so decode the
        // inner text like a reg-name but additionally allow ':'.
        let (h, _) = bracketed.split_once(']')?;
        decode_host(h, true)?
    } else {
        let h = match host_port.rsplit_once(':') {
            Some((h, port)) => {
                // RFC 3986 port must be digits (and may be empty). A
                // non-numeric port fails Swift's URL parse → host nil → WAN.
                if port.bytes().all(|b| b.is_ascii_digit()) {
                    h
                } else {
                    return None;
                }
            }
            None => host_port,
        };
        decode_host(h, false)?
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

/// Validates a raw host the way Foundation's URL parser does and returns the
/// percent-decoded text (what Swift's `.host` yields). ASCII characters must
/// be RFC 3986 reg-name members — alphanumeric, `-._~`, sub-delims
/// `!$&'()*+,;=` — or a well-formed `%XX` escape; anything else (space,
/// `\`, `^`, `|`, `{`, `<`, `"`, backtick, a bare `%`, …) makes Foundation
/// return a nil URL → `None` here. Non-ASCII is passed through (Foundation
/// IDNA-encodes it instead; see `extract_host` for the accepted residual
/// divergence). Decoded bytes must form valid UTF-8 — Foundation also
/// returns a nil host for e.g. `http://%ff.local`.
fn decode_host(raw: &str, allow_colon: bool) -> Option<String> {
    fn hex_val(b: u8) -> Option<u8> {
        (b as char).to_digit(16).map(|v| v as u8)
    }
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let hi = bytes.get(i + 1).copied().and_then(hex_val)?;
            let lo = bytes.get(i + 2).copied().and_then(hex_val)?;
            out.push(hi * 16 + lo);
            i += 3;
        } else if b.is_ascii_alphanumeric()
            || b"-._~!$&'()*+,;=".contains(&b)
            || (allow_colon && b == b':')
            || !b.is_ascii()
        {
            out.push(b);
            i += 1;
        } else {
            return None;
        }
    }
    String::from_utf8(out).ok()
}

/// Returns the class for a dotted-quad IPv4 host, or `None` when `host` is
/// not a numeric IPv4 literal (so the caller treats it as a hostname). Port
/// of `ServerConfig.classifyIPv4` in `ServerConfig.swift`.
///
/// Swift splits on "." WITHOUT omitting empty subsequences and parses each
/// part with `Int(_:)` (which tolerates leading zeros and a leading '+',
/// just like Rust's `u32::from_str`), requiring 0...255. Exactly 4 parts or
/// it is not an IPv4 literal.
fn classify_ipv4(host: &str) -> Option<ServerUrlClass> {
    let mut octets = [0u32; 4];
    let mut count = 0usize;
    // `str::split` keeps empty parts, matching Swift's
    // `omittingEmptySubsequences: false` — "1..2.3" splits into 4 parts but
    // the empty part fails to parse, so the host falls through to hostname
    // handling (→ WAN), same as in Swift.
    for part in host.split('.') {
        if count == 4 {
            return None; // more than 4 parts → hostname
        }
        let v: u32 = part.parse().ok()?;
        if v > 255 {
            return None;
        }
        octets[count] = v;
        count += 1;
    }
    if count != 4 {
        return None;
    }
    let (a, b) = (octets[0], octets[1]);
    // BYTE-CRITICAL range bounds (checklist B traps): 100.64.0.0/10 ends at
    // 100.127.255.255; 172.16.0.0/12 ends at 172.31.255.255 (172.32 → WAN).
    if a == 100 && (64..=127).contains(&b) {
        return Some(ServerUrlClass::Tailscale); // 100.64.0.0/10
    }
    if a == 10 {
        return Some(ServerUrlClass::Lan); // 10.0.0.0/8
    }
    if a == 172 && (16..=31).contains(&b) {
        return Some(ServerUrlClass::Lan); // 172.16.0.0/12
    }
    if a == 192 && b == 168 {
        return Some(ServerUrlClass::Lan); // 192.168.0.0/16
    }
    if a == 169 && b == 254 {
        return Some(ServerUrlClass::Lan); // 169.254.0.0/16
    }
    Some(ServerUrlClass::Wan)
}

// ─── §5.3 network try-order ─────────────────────────────────────────────

/// §5.3 — the preferred URL-class order for `network`, or `None` when the
/// network gives no useful signal (keep the publisher's order). Port of
/// `ServerConfig.classPreference` in `ServerConfig.swift`.
///
/// BYTE-CRITICAL try-orders (checklist B):
/// - on Wi-Fi (even with Tailscale also up): `[lan, tailscale, wan]`
/// - off Wi-Fi with Tailscale up:            `[tailscale, wan, lan]`
/// - plain cellular:                         `[wan, tailscale, lan]`
/// - no signal:                              `None` (original order)
///
/// A `Some` SSID alone counts as "on Wi-Fi" — it is the fallback wifi
/// signal for callers that do not populate `is_wifi`.
pub fn class_preference(network: &NetworkContext) -> Option<[ServerUrlClass; 3]> {
    let on_wifi = network.is_wifi || network.ssid.is_some();
    if on_wifi {
        return Some([
            ServerUrlClass::Lan,
            ServerUrlClass::Tailscale,
            ServerUrlClass::Wan,
        ]);
    }
    if network.is_tailscale {
        return Some([
            ServerUrlClass::Tailscale,
            ServerUrlClass::Wan,
            ServerUrlClass::Lan,
        ]);
    }
    if network.is_cellular {
        return Some([
            ServerUrlClass::Wan,
            ServerUrlClass::Tailscale,
            ServerUrlClass::Lan,
        ]);
    }
    None
}

/// §5.3 — a config's candidate URLs re-ordered for `network`. Port of
/// `ServerConfig.orderedURLs(network:)` in `ServerConfig.swift`.
///
/// BYTE-CRITICAL: this is a STABLE sort — URLs of a more-preferred class
/// move ahead, but within one class (and when the network gives no signal)
/// the publisher's original order is preserved. Reachability is NOT
/// consulted here; this only decides the *try order*.
pub fn ordered_urls(urls: &[String], network: &NetworkContext) -> Vec<String> {
    let Some(preference) = class_preference(network) else {
        return urls.to_vec();
    };
    let rank = |u: &str| -> usize {
        preference
            .iter()
            .position(|c| *c == classify_url(u))
            .unwrap_or(preference.len())
    };
    let mut sorted = urls.to_vec();
    // `Vec::sort_by_key` is a stable sort, matching the Swift source's
    // explicit (rank, original-offset) tiebreak.
    sorted.sort_by_key(|u| rank(u));
    sorted
}

/// §5.3 — the try-order with the probe's verdict layered on top. Port of
/// `ServerConfig.preferredURLs(live:network:)` in `ServerConfig.swift`.
///
/// A `live` URL the last probe confirmed reachable leads; the remaining
/// candidates follow in shape order as fallbacks. BYTE-CRITICAL: a `live`
/// value not present in `urls` (the config was edited since the probe wrote
/// it) is IGNORED rather than resurrected — the result is plain shape order.
pub fn preferred_urls(
    urls: &[String],
    live: Option<&str>,
    network: &NetworkContext,
) -> Vec<String> {
    let ordered = ordered_urls(urls, network);
    let Some(live) = live else {
        return ordered;
    };
    if !urls.iter().any(|u| u == live) {
        return ordered;
    }
    let mut out = Vec::with_capacity(ordered.len());
    out.push(live.to_string());
    // Swift: `[live] + ordered.filter { $0 != live }` — drops EVERY
    // occurrence equal to `live`, not just the first.
    out.extend(ordered.into_iter().filter(|u| u != live));
    out
}

// ─── §5.2 active-config resolution ──────────────────────────────────────

/// §5.2 — resolve which config is active, as a pure function over plain
/// inputs. Port of `ServerConfigList.activeConfig` in `ServerConfig.swift`,
/// returning the index into `config_ids` instead of the config itself.
///
/// BYTE-CRITICAL: a stale/unknown `active_config_id` falls back to index 0
/// (the first config); only an empty list yields `None`. The FIRST config
/// whose id equals `active_config_id` wins (Swift `first(where:)`).
///
/// Swift's `ServerConfigList.effectiveActiveConfig(network:)` is exactly
/// this composed with [`ordered_urls`] over the resolved config's URLs.
pub fn resolve_active_index(
    config_ids: &[String],
    active_config_id: Option<&str>,
) -> Option<usize> {
    if config_ids.is_empty() {
        return None;
    }
    if let Some(id) = active_config_id {
        if let Some(hit) = config_ids.iter().position(|c| c == id) {
            return Some(hit);
        }
    }
    Some(0)
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper mirroring `FixturesTests.net(_:wifi:cellular:tailscale:)`.
    fn net(ssid: Option<&str>, wifi: bool, cellular: bool, tailscale: bool) -> NetworkContext {
        NetworkContext {
            ssid: ssid.map(str::to_string),
            is_wifi: wifi,
            is_cellular: cellular,
            is_tailscale: tailscale,
        }
    }

    fn urls(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Mirrors `FixturesTests.multiURL`: a profile reachable at
    /// [wan, lan, tailscale] in publisher order.
    fn multi_url() -> Vec<String> {
        urls(&[
            "https://wan.example.com",
            "http://192.168.1.10:5033",
            "http://100.64.0.8:5033",
        ])
    }

    // ── §5.1 URL classification (host shape) ──────────────────────────

    #[test]
    fn classify_url_golden_vectors() {
        // Ported verbatim from Swift `FixturesTests.test_classifyURL`.
        assert_eq!(
            classify_url("http://192.168.1.5:42720"),
            ServerUrlClass::Lan
        );
        assert_eq!(classify_url("http://10.0.0.5"), ServerUrlClass::Lan);
        assert_eq!(classify_url("http://172.16.3.4"), ServerUrlClass::Lan);
        // Swift comment: "outside the /12"
        assert_eq!(classify_url("http://172.32.0.1"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://169.254.1.1"), ServerUrlClass::Lan);
        assert_eq!(classify_url("https://nas.local:5033"), ServerUrlClass::Lan);
        assert_eq!(
            classify_url("http://100.64.0.5:42720"),
            ServerUrlClass::Tailscale
        );
        // Swift comment: "outside the /10"
        assert_eq!(classify_url("http://100.128.0.5"), ServerUrlClass::Wan);
        assert_eq!(
            classify_url("https://box.tail-abc.ts.net"),
            ServerUrlClass::Tailscale
        );
        assert_eq!(
            classify_url("https://203-0-113-10.sslip.io"),
            ServerUrlClass::Wan
        );
        assert_eq!(classify_url("https://8.8.8.8"), ServerUrlClass::Wan);
    }

    #[test]
    fn classify_url_range_boundaries() {
        // Rust-side hardening for the checklist-B boundary traps (no Swift
        // vector pins these exact bounds; semantics follow
        // `ServerConfig.classifyIPv4`).
        assert_eq!(classify_url("http://172.31.255.255"), ServerUrlClass::Lan);
        assert_eq!(classify_url("http://172.15.0.1"), ServerUrlClass::Wan);
        assert_eq!(
            classify_url("http://100.127.255.255"),
            ServerUrlClass::Tailscale
        );
        assert_eq!(classify_url("http://100.63.255.255"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://10.255.255.255"), ServerUrlClass::Lan);
        assert_eq!(classify_url("http://192.169.0.1"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://169.253.1.1"), ServerUrlClass::Wan);
    }

    #[test]
    fn classify_url_suffix_must_include_dot() {
        // Rust-side hardening: `hasSuffix(".ts.net")` must not match a host
        // merely ending in "ts.net" without the dot boundary.
        assert_eq!(classify_url("https://fakets.net"), ServerUrlClass::Wan);
        assert_eq!(classify_url("https://x.fakets.net"), ServerUrlClass::Wan);
        assert_eq!(classify_url("https://ts.net"), ServerUrlClass::Wan);
        assert_eq!(classify_url("https://notlocal"), ServerUrlClass::Wan);
        // Case-insensitive host (Swift lowercases the host before matching).
        assert_eq!(classify_url("https://NAS.LOCAL"), ServerUrlClass::Lan);
        assert_eq!(
            classify_url("https://Box.TS.Net"),
            ServerUrlClass::Tailscale
        );
    }

    #[test]
    fn classify_url_parsing_edge_cases() {
        // Rust-side hardening mirroring Swift `URL(string:)?.host` fallbacks:
        // unparsable / host-less inputs classify as WAN.
        assert_eq!(classify_url(""), ServerUrlClass::Wan);
        // No scheme → Swift host is nil (the text parses as a path) → WAN.
        assert_eq!(classify_url("nas.local"), ServerUrlClass::Wan);
        assert_eq!(classify_url("not a url"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://"), ServerUrlClass::Wan); // empty host
        assert_eq!(classify_url("http://[::1]:8080"), ServerUrlClass::Wan); // IPv6
        assert_eq!(classify_url("  http://10.0.0.5  "), ServerUrlClass::Lan); // trimmed
        assert_eq!(
            classify_url("http://user:pass@192.168.1.5:8080/path?q=1"),
            ServerUrlClass::Lan, // userinfo + port + path ignored
        );
        // 5 dotted parts / out-of-range octet → hostname, not IPv4 → WAN.
        assert_eq!(classify_url("http://1.2.3.4.5"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://256.1.1.1"), ServerUrlClass::Wan);
    }

    #[test]
    fn classify_url_foundation_host_semantics() {
        // Rust-side hardening: every expectation below was verified against
        // Swift 6.2 Foundation (`URL(string:)?.host?.lowercased()` fed into
        // the Swift classifier) — invalid host characters / escapes make the
        // URL nil → WAN, while `.host` percent-decodes valid escapes.
        assert_eq!(classify_url("http://my nas.local"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://foo\\bar.local"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://a|b.local"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://a^b.local"), ServerUrlClass::Wan);
        assert_eq!(classify_url("http://ex%zz.local"), ServerUrlClass::Wan); // bad escape
        assert_eq!(classify_url("http://x%"), ServerUrlClass::Wan); // truncated escape
        assert_eq!(classify_url("http://%ff.local"), ServerUrlClass::Wan); // non-UTF-8 decode
        assert_eq!(
            classify_url("http://192.168.1.5:80:80"), // double port → nil URL
            ServerUrlClass::Wan
        );
        // `.host` is percent-DECODED: the decoded text is what classifies.
        assert_eq!(classify_url("http://x.ts%2Enet"), ServerUrlClass::Tailscale);
        assert_eq!(classify_url("http://nas%2Elocal"), ServerUrlClass::Lan);
        // "%31%30.0.0.5" decodes to "10.0.0.5", "1%392.168.1.5" to
        // "192.168.1.5", "a%20b.local" to "a b.local" (still ".local").
        assert_eq!(classify_url("http://%31%30.0.0.5"), ServerUrlClass::Lan);
        assert_eq!(classify_url("http://1%392.168.1.5"), ServerUrlClass::Lan);
        assert_eq!(classify_url("http://a%20b.local"), ServerUrlClass::Lan);
        // reg-name extras Foundation accepts.
        assert_eq!(
            classify_url("http://under_score.local"),
            ServerUrlClass::Lan
        );
        assert_eq!(classify_url("http://tilde~.local"), ServerUrlClass::Lan);
        // Bracketed hosts: Foundation strips the brackets leniently — the
        // inner text classifies as-is.
        assert_eq!(classify_url("http://[10.0.0.5]"), ServerUrlClass::Lan);
        assert_eq!(classify_url("http://[x.local]"), ServerUrlClass::Lan);
        // Invalid userinfo never invalidates the host (Foundation drops it).
        assert_eq!(classify_url("http://a%zz b@nas.local"), ServerUrlClass::Lan);
    }

    // ── §5.1 SSID normalization ────────────────────────────────────────

    #[test]
    fn normalize_ssid_strips_quotes_and_rejects_placeholders() {
        // Ported verbatim from Swift
        // `FixturesTests.test_serverConfig_normalizeSSID_stripsQuotesAndRejectsPlaceholders`.
        assert_eq!(
            normalize_ssid(Some("\"Home-5G\"")),
            Some("Home-5G".to_string())
        );
        assert_eq!(
            normalize_ssid(Some("  Home-5G  ")),
            Some("Home-5G".to_string())
        );
        assert_eq!(normalize_ssid(Some("")), None);
        assert_eq!(normalize_ssid(None), None);
        assert_eq!(normalize_ssid(Some("<unknown ssid>")), None);
        assert_eq!(normalize_ssid(Some("0x")), None);
    }

    #[test]
    fn normalize_ssid_strips_exactly_one_quote_layer() {
        // Rust-side hardening: Swift strips ONE surrounding quote pair only,
        // then trims, then applies the placeholder/empty check.
        assert_eq!(
            normalize_ssid(Some("\"\"Home\"\"")),
            Some("\"Home\"".to_string())
        );
        // Inner trim after the quote strip.
        assert_eq!(
            normalize_ssid(Some("\"  Home \"")),
            Some("Home".to_string())
        );
        // A bare quote pair strips to empty → None.
        assert_eq!(normalize_ssid(Some("\"\"")), None);
        // A lone quote char (count < 2) is not stripped and survives.
        assert_eq!(normalize_ssid(Some("\"")), Some("\"".to_string()));
        // Quoted placeholders normalize to None (check runs after stripping).
        assert_eq!(normalize_ssid(Some("\"<unknown ssid>\"")), None);
        assert_eq!(normalize_ssid(Some("\"0x\"")), None);
        // Leading-quote-only string is NOT stripped (no matching suffix).
        assert_eq!(normalize_ssid(Some("\"Home")), Some("\"Home".to_string()));
    }

    // ── §5.3 network-based URL ordering ────────────────────────────────

    #[test]
    fn ordered_urls_on_wifi_prefers_lan() {
        // Swift `FixturesTests.test_orderedURLs_onWifiPrefersLAN`.
        assert_eq!(
            ordered_urls(&multi_url(), &net(None, true, false, false)),
            urls(&[
                "http://192.168.1.10:5033",
                "http://100.64.0.8:5033",
                "https://wan.example.com",
            ])
        );
    }

    #[test]
    fn ordered_urls_ssid_name_alone_counts_as_wifi() {
        // Swift `FixturesTests.test_orderedURLs_ssidNameAloneCountsAsWifi`:
        // a readable SSID implies on-Wi-Fi even when `is_wifi` is false.
        let ordered = ordered_urls(&multi_url(), &net(Some("Home"), false, false, false));
        assert_eq!(
            ordered.first().map(String::as_str),
            Some("http://192.168.1.10:5033")
        );
    }

    #[test]
    fn ordered_urls_off_wifi_tailscale_leads() {
        // Swift `FixturesTests.test_orderedURLs_offWifiTailscaleLeads`:
        // off Wi-Fi with Tailscale up → TS leads, then WAN, then LAN last.
        assert_eq!(
            ordered_urls(&multi_url(), &net(None, false, false, true)),
            urls(&[
                "http://100.64.0.8:5033",
                "https://wan.example.com",
                "http://192.168.1.10:5033",
            ])
        );
    }

    #[test]
    fn ordered_urls_on_wifi_lan_beats_tailscale() {
        // Swift `FixturesTests.test_orderedURLs_onWifiLANBeatsTailscale`:
        // on Wi-Fi the LAN path wins even when Tailscale is also up.
        assert_eq!(
            ordered_urls(&multi_url(), &net(None, true, false, true)),
            urls(&[
                "http://192.168.1.10:5033",
                "http://100.64.0.8:5033",
                "https://wan.example.com",
            ])
        );
    }

    #[test]
    fn ordered_urls_on_cellular_prefers_wan() {
        // Swift `FixturesTests.test_orderedURLs_onCellularPrefersWAN`.
        assert_eq!(
            ordered_urls(&multi_url(), &net(None, false, true, false)),
            urls(&[
                "https://wan.example.com",
                "http://100.64.0.8:5033",
                "http://192.168.1.10:5033",
            ])
        );
    }

    #[test]
    fn ordered_urls_no_signal_keeps_publisher_order() {
        // Swift `FixturesTests.test_orderedURLs_noSignalKeepsPublisherOrder`.
        assert_eq!(
            ordered_urls(&multi_url(), &net(None, false, false, false)),
            multi_url()
        );
    }

    #[test]
    fn ordered_urls_stable_within_class() {
        // Swift `FixturesTests.test_orderedURLs_stableWithinClass`: two LAN
        // URLs keep their original relative order on Wi-Fi.
        let cfg_urls = urls(&[
            "http://192.168.1.2",
            "https://wan.example.com",
            "http://192.168.1.3",
        ]);
        assert_eq!(
            ordered_urls(&cfg_urls, &net(None, true, false, false)),
            urls(&[
                "http://192.168.1.2",
                "http://192.168.1.3",
                "https://wan.example.com",
            ])
        );
    }

    #[test]
    fn ordered_urls_from_fixture_multi_url_config() {
        // Swift `FixturesTests.test_orderedURLs_fromFixtureMultiURLConfig`,
        // with the "Home NAS" config from the iOS fixture
        // `docs/examples/server_config_list.json` inlined:
        // [wan, lan, tailscale] in publisher order.
        let home_nas = urls(&[
            "https://203-0-113-10.sslip.io",
            "http://192.168.1.10:5033",
            "http://100.64.0.8:5033",
        ]);
        assert_eq!(
            ordered_urls(&home_nas, &net(None, true, false, false))
                .first()
                .map(String::as_str),
            Some("http://192.168.1.10:5033")
        );
        assert_eq!(
            ordered_urls(&home_nas, &net(None, false, false, true))
                .first()
                .map(String::as_str),
            Some("http://100.64.0.8:5033")
        );
    }

    // ── §5.3 preferredURLs (probe verdict layered on shape order) ──────

    #[test]
    fn preferred_urls_live_leads_rest_follow_shape_order() {
        // Swift `FixturesTests.test_preferredURLs_liveLeadsRestFollowShapeOrder`:
        // live = WAN (last probe ran on cellular), now on Wi-Fi — the
        // confirmed URL still leads, fallbacks in Wi-Fi shape order.
        assert_eq!(
            preferred_urls(
                &multi_url(),
                Some("https://wan.example.com"),
                &net(None, true, false, false)
            ),
            urls(&[
                "https://wan.example.com",
                "http://192.168.1.10:5033",
                "http://100.64.0.8:5033",
            ])
        );
    }

    #[test]
    fn preferred_urls_live_already_first_is_identity() {
        // Swift `FixturesTests.test_preferredURLs_liveAlreadyFirstIsIdentity`.
        let wifi = net(None, true, false, false);
        assert_eq!(
            preferred_urls(&multi_url(), Some("http://192.168.1.10:5033"), &wifi),
            ordered_urls(&multi_url(), &wifi)
        );
    }

    #[test]
    fn preferred_urls_nil_live_falls_back_to_shape_order() {
        // Swift `FixturesTests.test_preferredURLs_nilLiveFallsBackToShapeOrder`.
        let cell = net(None, false, true, false);
        assert_eq!(
            preferred_urls(&multi_url(), None, &cell),
            ordered_urls(&multi_url(), &cell)
        );
    }

    #[test]
    fn preferred_urls_stale_live_not_in_urls_is_ignored() {
        // Swift `FixturesTests.test_preferredURLs_staleLiveNotInUrlsIsIgnored`:
        // the config was edited since the probe wrote the live URL — the
        // removed candidate must not resurrect.
        let wifi = net(None, true, false, false);
        assert_eq!(
            preferred_urls(&multi_url(), Some("http://10.0.0.9:5033"), &wifi),
            ordered_urls(&multi_url(), &wifi)
        );
    }

    // ── §5.2 active-config resolution ──────────────────────────────────

    #[test]
    fn active_config_falls_back_to_first_when_id_is_stale() {
        // Swift `FixturesTests.test_activeConfig_fallsBackToFirstWhenIdIsStale`.
        let ids = urls(&["alpha"]);
        assert_eq!(resolve_active_index(&ids, Some("stale-id")), Some(0));
    }

    #[test]
    fn active_config_is_none_when_configs_is_empty() {
        // Swift `FixturesTests.test_activeConfig_isNilWhenConfigsIsEmpty`.
        assert_eq!(resolve_active_index(&[], None), None);
        assert_eq!(resolve_active_index(&[], Some("anything")), None);
    }

    #[test]
    fn active_config_matches_id_when_present() {
        // Swift `FixturesTests.test_serverConfigList_decodesThreeConfigs`
        // (the resolution half): the fixture's activeConfigId picks the
        // third config (ids from docs/examples/server_config_list.json).
        let ids = urls(&[
            "0c1f2e3a-4b5c-6d7e-8f90-123456789abc",
            "11223344-5566-7788-99aa-bbccddeeff00",
            "ff112233-4455-6677-8899-aabbccddeeff",
        ]);
        assert_eq!(
            resolve_active_index(&ids, Some("ff112233-4455-6677-8899-aabbccddeeff")),
            Some(2)
        );
    }

    // ── §5.3 effectiveActiveConfig composition ─────────────────────────
    // Swift's `ServerConfigList.effectiveActiveConfig(network:)` is exactly
    // resolve_active_index → ordered_urls; these tests port its vectors
    // through that composition.

    #[test]
    fn effective_active_config_reorders_active_config_urls() {
        // Swift `FixturesTests.test_effectiveActiveConfig_reordersActiveConfigUrls`.
        let ids = urls(&["m"]);
        assert_eq!(resolve_active_index(&ids, Some("m")), Some(0));
        let eff = ordered_urls(&multi_url(), &net(None, true, false, false));
        assert_eq!(
            eff.first().map(String::as_str),
            Some("http://192.168.1.10:5033"),
            "effective url is the network-preferred candidate"
        );
    }

    #[test]
    fn effective_active_config_keeps_active_profile_never_crosses_profiles() {
        // Swift `FixturesTests.test_effectiveActiveConfig_keepsActiveProfileNeverCrossesProfiles`:
        // auto-switch never crosses profiles — the active profile stays put
        // on a Tailscale network, only its own URLs reorder.
        let ids = urls(&["a", "m"]);
        assert_eq!(resolve_active_index(&ids, Some("a")), Some(0));
    }

    #[test]
    fn effective_active_config_none_when_empty() {
        // Swift `FixturesTests.test_effectiveActiveConfig_nilWhenEmpty`.
        assert_eq!(resolve_active_index(&[], None), None);
    }

    #[test]
    fn effective_active_config_from_fixture_keeps_active_profile_across_networks() {
        // Swift `FixturesTests.test_effectiveActiveConfig_fromFixtureKeepsActiveProfileAcrossNetworks`,
        // with docs/examples/server_config_list.json inlined. The active
        // profile (#3) is the user's pick and never changes with the
        // network; it is single-URL so its effective urls are identical on
        // every network.
        let ids = urls(&[
            "0c1f2e3a-4b5c-6d7e-8f90-123456789abc",
            "11223344-5566-7788-99aa-bbccddeeff00",
            "ff112233-4455-6677-8899-aabbccddeeff",
        ]);
        let remote_id = Some("ff112233-4455-6677-8899-aabbccddeeff");
        let remote_urls = urls(&["https://clip.example.com"]);
        for network in [
            net(None, true, false, false),
            net(None, false, true, false),
            net(None, false, false, false),
        ] {
            assert_eq!(resolve_active_index(&ids, remote_id), Some(2));
            assert_eq!(ordered_urls(&remote_urls, &network), remote_urls);
        }
    }

    // ── class_preference (try-order table) ─────────────────────────────

    #[test]
    fn class_preference_try_order_table() {
        // Rust-side hardening pinning the §5.3 table from
        // `ServerConfig.classPreference` (checklist B try-order row).
        use ServerUrlClass::{Lan, Tailscale, Wan};
        assert_eq!(
            class_preference(&net(None, true, false, false)),
            Some([Lan, Tailscale, Wan])
        );
        assert_eq!(
            class_preference(&net(None, true, true, true)),
            Some([Lan, Tailscale, Wan]),
            "Wi-Fi wins over every other signal"
        );
        assert_eq!(
            class_preference(&net(Some("Home"), false, false, false)),
            Some([Lan, Tailscale, Wan]),
            "SSID alone counts as Wi-Fi"
        );
        assert_eq!(
            class_preference(&net(None, false, true, true)),
            Some([Tailscale, Wan, Lan]),
            "Tailscale beats cellular off Wi-Fi"
        );
        assert_eq!(
            class_preference(&net(None, false, true, false)),
            Some([Wan, Tailscale, Lan])
        );
        assert_eq!(class_preference(&net(None, false, false, false)), None);
    }
}
