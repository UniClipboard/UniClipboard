# PRD: Multi-URL Per-Server Auto-Switch — Android (Flutter) Client

| | |
|---|---|
| **Status** | Draft for implementation |
| **Date** | 2026-06-11 |
| **Author** | UniClipboard iOS team (ported from the shipped iOS implementation) |
| **Audience** | Android/Flutter client implementer (human or agent) |
| **Normative source** | [`docs/SYNC_PROTOCOL.md`](./SYNC_PROTOCOL.md) §5.1–§5.6 — when this PRD and the spec disagree, **the spec wins** |
| **Reference implementation** | UniClipboard iOS, commit `828da9d` (`feat: multi-URL per-server auto-switch via reachability probing`) |

---

## 1. Summary

Replace the Android client's per-config network auto-switch (a strategy +
SSID list attached to each `ServerConfig`, selecting **between** profiles)
with **multiple candidate URLs inside one profile**, selected by network
shape + reachability probing (selecting **between a profile's URLs**).

One self-hosted server is typically reachable over several paths — LAN IP,
Tailscale address, public relay/domain. Today the user must either pick one
URL (and lose connectivity when they leave that network) or duplicate the
profile per network and maintain fragile SSID rules. After this change, one
profile carries an ordered URL candidate list and the client automatically
uses the best reachable path for the current network, with zero per-network
configuration.

This is a **lockstep port** of the behavior already shipped in the iOS
client. All protocol-level decisions are settled; do not re-litigate them.
Platform-specific mappings (ConnectivityManager, WifiManager, cleartext
policy, Dio) are specified in §8.

## 2. Background & motivation

- The old model conflated two ideas: *which server identity* (credentials)
  and *which network path*. Users with one server on three paths had to
  clone the profile three times.
- SSID-based rules are brittle on both platforms: Android requires
  `ACCESS_FINE_LOCATION` + Location Services ON just to read the SSID, and
  returns `<unknown ssid>` placeholders when it can't. iOS has the same
  class of problem (entitlement + Location auth).
- The desktop pairing QR now publishes the full candidate list (`urls`), so
  a freshly paired mobile client can auto-switch from the first sync.

## 3. Goals

1. `ServerConfig` carries ordered candidate base URLs (`urls`); `url` stays
   as the legacy mirror of `urls[0]` (§5.1).
2. The client automatically uses the best **reachable** candidate for the
   current network, re-evaluating on network change / app foreground /
   sync failure, without user interaction.
3. Auto-switch **never changes the active profile**. `activeConfigId`
   remains the user's manual pick (§5.2).
4. Old persisted data (single `url`, `autoSwitchStrategy`,
   `autoSwitchWifiNames`) migrates losslessly and silently.
5. Pairing QR / connect URI `urls` flows into the new profile.
6. Editor UI supports the candidate list; "Test connection" probes **all**
   candidates and shows per-URL reachability plus which one the current
   network will use.
7. Cross-platform config import/export keeps working (same JSON shape on
   both platforms, byte-compatible with `docs/examples/server_config_list.json`).

### Non-goals

- No changes to the wire protocol (§1–§4 of the spec) or the server.
- No per-URL credentials. One profile = one credential pair.
- No background/periodic probing beyond the listed triggers (§6.4). Battery
  matters; the steady-state probe *is* the sync request itself.
- No latency-based racing between two reachable candidates (deterministic
  shape-order pick, see §6.3 — this is a settled decision).
- No automatic profile switching of any kind (the old strategy concept is
  removed, not reworked).

## 4. Glossary

| Term | Meaning |
|---|---|
| **Candidate URL** | One entry of `ServerConfig.urls` — a complete base URL |
| **URL class** | Host-shape classification: `lan` / `tailscale` / `wan` (§5.1) |
| **Shape order** (Layer 1) | Pure, I/O-free re-ordering of `urls` for the current network |
| **Probe** (Layer 2) | Short-timeout `GET /SyncClipboard.json` per candidate to test reachability |
| **Live URL** | The probe-confirmed candidate persisted per profile (`{configId: url}`) |
| **NetworkContext** | Snapshot: `ssid?`, `isWifi`, `isCellular`, `isTailscale` |

## 5. Data model (normative — spec §5.1/§5.2)

### 5.1 `ServerConfig`

```jsonc
{
  "id":       "uuid-v4-string",
  "name":     "string | null",
  "url":      "https://...",                  // == urls[0], legacy mirror
  "urls":     ["https://...", "http://..."],  // ordered candidates, never empty
  "username": "string",
  "password": "string"
}
```

Codec rules (all asserted by iOS unit tests; replicate them):

- **Decode**: `urls` non-empty → source of truth. `urls` absent/empty →
  `urls = [url]`. Both absent → decode error (invalid config).
- **Decode**: `autoSwitchStrategy` / `autoSwitchWifiNames` are
  **decoded-and-dropped** — tolerated on read, never re-encoded, no error.
- **Encode**: write **both** keys, `url` always `== urls[0]`, so an old
  single-URL reader (or an old app version after downgrade) still works.
- Readers MUST tolerate a trailing slash on any candidate.

### 5.2 `ServerConfigList`

Unchanged shape (`configs` + `activeConfigId`). Reminders:

- Stale `activeConfigId` falls back to `configs[0]`; empty `configs` means
  **no network calls allowed**.
- If the Android client ever had a client-private "override/pin" key
  analogous to iOS's `manualOverrideConfigId`, apply the same migration:
  promote a resolvable pin into `activeConfigId` on read, never re-encode.

### 5.3 URL classification

From the **host alone** — no DNS, no probing. Hostname suffix checks win
over numeric parsing:

| Class | Host matches |
|---|---|
| `tailscale` | IPv4 in `100.64.0.0/10`, or `*.ts.net` |
| `lan` | IPv4 in `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, or `*.local` |
| `wan` | everything else (public IP, any other hostname, unparseable host) |

### 5.4 SSID normalization (§5.1)

Keep the shared rule even though SSIDs are no longer matched (the value
survives as a "which Wi-Fi" signal): trim → strip wrapping ASCII quotes →
reject `<unknown ssid>` and `0x` → empty ⇒ "no SSID". Android is the
platform the quote rule exists for.

## 6. Resolution algorithm (normative — spec §5.3)

### 6.1 NetworkContext

```
NetworkContext = { ssid: string?, isWifi: bool, isCellular: bool, isTailscale: bool }
```

- `isWifi` / `isCellular`: from the OS connectivity API (no permission).
- `isTailscale`: true iff a local interface holds an IPv4 in
  `100.64.0.0/10` (interface enumeration, no permission). This pins
  "Tailscale up" rather than "some VPN up"; the false-positive (a custom
  CGNAT VPN) is acceptable.
- `ssid`: optional. **Only a fallback Wi-Fi signal** for contexts that
  can't populate `isWifi`. Do NOT add a location-permission prompt just to
  read it — `isWifi` from the transport API makes it unnecessary for the
  resolver.

### 6.2 Layer 1 — shape ordering (pure, no I/O)

```
classPreference(network):
    onWifi = network.isWifi or network.ssid != null
    if onWifi:              return [lan, tailscale, wan]
    if network.isTailscale: return [tailscale, wan, lan]
    if network.isCellular:  return [wan, tailscale, lan]
    return null                       # no signal → keep publisher order

orderedURLs(cfg, network):
    pref = classPreference(network)
    if pref == null: return cfg.urls
    return stableSort(cfg.urls, key = indexOrEnd(pref, classify(url)))
```

Settled decisions baked into that table:

- **On Wi-Fi, LAN beats Tailscale** even when Tailscale is up — the direct
  LAN path is lowest-latency, and probing demotes a LAN URL that doesn't
  work on a foreign Wi-Fi, so ranking it first is safe.
- The sort is **stable**: within one class (and with no network signal) the
  publisher's order is preserved — `urls[0]` is the publisher's default.

### 6.3 Layer 2 — reachability probing

Probe semantics (each candidate, concurrently):

- One `GET <base>/SyncClipboard.json`, **~2 s timeout**, no retry, no body
  decode, never "wait for connectivity".
- **404 = reachable** (server up, clipboard empty — §2.1).
- **401 = reachable** (path works; bad credentials are an account problem
  the sync engine surfaces separately — the picker must not skip a working
  direct path because the password is stale).
- Everything else (timeout, connection refused, 5xx, TLS failure) =
  unreachable.

Pick: **first reachable in shape order** — deterministic given the probe
results, NOT a connection race. Two reachable candidates resolve to
whichever ranks earlier. All-unreachable ⇒ live URL cleared (readers fall
back to pure shape order, i.e. `orderedURLs[0]`).

Effective try-order (`preferredURLs`): the live URL leads, the remaining
candidates follow in shape order as fallbacks. A persisted live URL no
longer present in `urls` (config edited since the probe) MUST be ignored,
not resurrected.

### 6.4 Probe triggers (exhaustive — nothing else probes)

1. **Profile switch** (manual pick changed) — forced.
2. **Network change** (connectivity callback) — forced.
3. **App foreground** — forced.
4. **Sync-loop network failure** (unreachable/timeout classes only, not
   auth/protocol errors) — debounced.
5. **User taps "Test connection"** — probes all candidates, drives the UI,
   and seeds the live URL with the verdict.

Orchestration requirements (mirror `AppViewModel.refreshLiveEndpoint`):

- **In-flight dedup**: a second trigger while a probe runs awaits the
  running probe instead of stacking another.
- **Debounce** non-forced triggers (iOS: 10 s). The sync loop retries
  failures every second; without the debounce each failing tick would burn
  a full probe round.
- **Stale-profile guard**: if the user switches profiles while a probe is
  in flight, discard the result (it describes the old profile).
- A successful sync request is the implicit confirmation of the current
  live URL — no probing on the happy path.

### 6.5 Live URL persistence

- Shape: `{configId: url}` map, one entry per profile.
- Cleared per-profile when a probe finds nothing reachable.
- iOS stores it as an atomically-written JSON **file** in the App Group
  because its keyboard/share extensions read it cross-process and
  `cfprefsd` caches per-process. On Android, if the client is
  single-process, plain `SharedPreferences` is fine; if any secondary
  process/consumer exists (widget, IME, tile service), match the iOS
  pattern: only the main app writes, consumers read, storage must be
  cross-process-fresh.
- Hydrate the in-memory copy at startup — the previous launch's verdict is
  the best first guess until the foreground probe lands.

### 6.6 Sync-engine integration

- The sync loop builds its HTTP client from the effective `urls[0]`
  (= `preferredURLs(live, network)[0]`) **per request/tick**, so a live-URL
  flip takes effect without restarting anything.
- **Switching URL within the same profile MUST NOT reset per-server sync
  state** (last-synced content hash, history watermark, throttle window).
  Same server ⇒ same content timeline. Only a *profile* change resets.
- On a network-class sync failure, request a (debounced) re-probe; do not
  rotate URLs inline in the request path.

## 7. Pairing payload / QR (spec §5.6 + connect-URI contract)

- The `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url>` payload
  carries optional `urls` (ordered, `url == urls[0]`; the desktop omits the
  field for single-URL pairing — Rust `skip_serializing_if = Vec::is_empty`).
- Parser rules (match iOS `ConnectURI` exactly; golden vector is shared
  across Swift/Rust/TS parsers): filter non-http(s) entries from `urls`;
  if all entries are dropped or the field is absent, fall back to `[url]`.
  The result is **always non-empty**.
- Accepting a pairing import creates the new profile with the **full**
  candidate list and makes it active.
- Legacy QR formats (plain JSON object, URL-with-userinfo) yield a
  single-element `urls`.

## 8. Android platform mapping (informative)

| Concern | iOS implementation | Android equivalent |
|---|---|---|
| `isWifi`/`isCellular` | `NWPathMonitor.usesInterfaceType` | `ConnectivityManager` + `NetworkCapabilities.TRANSPORT_WIFI` / `TRANSPORT_CELLULAR` (registerDefaultNetworkCallback; Flutter: `connectivity_plus`) |
| Tailscale detection | `getifaddrs`, IPv4 & `0xFFC00000 == 0x64400000` | `NetworkInterface.getNetworkInterfaces()` → any `Inet4Address` in `100.64.0.0/10` |
| Network-change trigger | `NWPathMonitor` callback | `ConnectivityManager.NetworkCallback` (`onAvailable`/`onCapabilitiesChanged`/`onLost`) |
| SSID (optional) | `NEHotspotNetwork.fetchCurrent` (entitlement + Location) | `WifiManager.connectionInfo.ssid` — requires `ACCESS_FINE_LOCATION` + Location ON on API 27+; returns quoted names and `<unknown ssid>` (hence §5.4). **Do not add the permission for this feature**; `isWifi` suffices |
| Probe HTTP client | dedicated `URLSession`, 2 s request+resource timeout, `waitsForConnectivity=false` | dedicated Dio instance: `connectTimeout`/`receiveTimeout` ≈ 2 s, no retry interceptor |
| Trust-insecure-cert | session delegate accepting any server trust (HTTPS only) | Dio `badCertificateCallback` / custom `SecurityContext` — same scope: **self-signed HTTPS only; plain HTTP is unaffected** (fix any UI copy that implies LAN/http needs it) |
| Cleartext HTTP | ATS lifted via `NSAllowsArbitraryLoads` (deliberately NOT `NSAllowsLocalNetworking`, which would re-block Tailscale CGNAT) | Android 9+ blocks cleartext by default: `android:usesCleartextTraffic="true"` or a Network Security Config that permits cleartext **for arbitrary hosts** — a `<domain-config>` allowlist cannot enumerate user-deployed servers, so the global flag is the correct shape |
| Persistence | App Group `UserDefaults` + files | `SharedPreferences`, same keys as §5.5 (`server_config_list`, `app_settings`) |
| Concurrency | Swift `withTaskGroup` | `Future.wait` over per-candidate Dio calls |

## 9. UI requirements

### 9.1 Server editor (add + edit)

- Replace the single URL field with an **ordered candidate list**: one row
  per URL, inline-editable, swipe/long-press to delete, "Add alternate
  address" appends a row. First row = default (publisher order).
- Each row with a parseable host shows a **class chip**: 局域网 / Tailscale
  / 公网 (LAN / Tailscale / Internet).
- Blank rows are allowed while editing but never persisted; a config's
  persisted `urls` is the trimmed, deduplicated, non-empty subset and MUST
  never be empty (block save / keep last valid).
- **Delete** all per-config auto-switch UI: strategy picker, SSID list
  editor, "current network → add SSID" row, and any list-row badge that
  rendered the old strategy.

### 9.2 Server list rows

- Multi-URL profiles show a summary line: candidate count + distinct
  classes in `lan, tailscale, wan` display order (e.g. “3 个地址 · 局域网 /
  Tailscale / 公网”). Single-URL profiles show nothing extra.

### 9.3 Test connection

- One tap probes **all** candidates concurrently (§6.3 semantics).
- Rows are listed in the current network's **shape order** (the try-order),
  each with a status icon: reachable ✓ / unreachable ✗ / reachable-but-bad-
  credentials (distinct icon — it's half good news).
- The candidate the resolver would pick gets a **“将使用” (Will use)**
  badge; a footer explains it re-selects automatically on network change.
- Editing any probe-relevant field (URLs, username, password, trust toggle)
  invalidates the displayed verdict.
- For an **existing** profile, a finished probe seeds the live-URL cache
  with the verdict (picked URL, or clears it when nothing is reachable).
  Draft profiles (add flow) don't seed — the post-save forced probe covers
  it. Invalidation-by-edit must NOT clear the persisted live URL.
- First-run/setup flow: gate completion on **at least one candidate
  returning success** (auth-failed alone is not enough to finish pairing).

### 9.4 Copy guidance

- Brand vocabulary: UniClipboard primary; SyncClipboard is a compatibility
  footnote.
- Trust-insecure-cert copy must say: needed only for **self-signed HTTPS**;
  plain HTTP doesn't need it; the setting is global.

## 10. Migration & compatibility

| Input | Behavior |
|---|---|
| Old config with `url` only | `urls = [url]`, silent |
| Old config with `autoSwitchStrategy`/`autoSwitchWifiNames` | decode-and-drop, silent; never re-encoded |
| New config read by an old app version | old reader sees `url` (== `urls[0]`) and works single-URL |
| Config export → other platform | identical JSON shape; round-trips against `docs/examples/server_config_list.json` |
| QR without `urls` | single-element candidate list |

No data-format version bump is needed; the change is additive + tolerant.

## 11. Acceptance criteria

1. Decoding `docs/examples/server_config_list.json` and re-encoding
   round-trips losslessly (both `url` and `urls` emitted; no `autoSwitch*`
   keys appear).
2. A config persisted by the pre-multi-URL Android build loads with
   `urls == [url]` and no error; saving it back drops the legacy
   auto-switch keys.
3. With profile `urls = [wan, lan, tailscale]`:
   - On Wi-Fi, try-order is `[lan, tailscale, wan]`; on cellular,
     `[wan, tailscale, lan]`; cellular+Tailscale, `[tailscale, wan, lan]`;
     no signal keeps publisher order. Within-class order is stable.
4. Probe maps 200→reachable, 404→reachable, 401→reachable-auth-failed,
   refused/timeout/5xx→unreachable; per-candidate timeout ≈ 2 s; all
   candidates probed concurrently.
5. With `urls[0]` dead and `urls[1]` alive (local stub), the sync loop
   recovers onto `urls[1]` within one debounce window without user action,
   **without** resetting the last-synced hash or history watermark, and the
   persisted live-URL map contains `{<configId>: <urls[1]>}`.
6. Wi-Fi → cellular transition triggers exactly one forced probe (in-flight
   dedup collapses the callback burst) and the effective URL flips to the
   WAN candidate.
7. Editing the profile to remove the current live URL does not resurrect
   it: next resolution uses shape order until a new probe lands.
8. "Test connection" on a 3-candidate profile shows three per-URL statuses,
   marks the picked one, and (edit flow) seeds the live-URL cache.
9. Scanning a connect URI whose payload carries 3 `urls` creates a profile
   with all 3 candidates; a legacy QR creates a single-candidate profile.
10. Cleartext `http://` to a LAN IP works on Android 9+ (cleartext policy
    configured); enabling/disabling trust-insecure-cert has no effect on
    plain-HTTP profiles and unblocks self-signed HTTPS ones.
11. No SSID/location permission prompt is introduced by this feature.

## 12. Test plan (mirror the iOS suites)

Unit (pure, no network):
- Codec: urls/url fallback, missing-both error, decode-and-drop, dual-key
  encode, trailing-slash tolerance (iOS: `FixturesTests`).
- `classifyURL`: each private range boundary (e.g. `172.15.x` → wan,
  `172.16.x` → lan; `100.63.x` → wan, `100.64.x` → tailscale), `*.local`,
  `*.ts.net`, hostname-over-numeric precedence, unparseable → wan.
- `orderedURLs`: the four network cases + stability within class.
- `preferredURLs`: live leads / nil live / stale live ignored / live
  already first is identity.
- Live-URL store: round-trip, per-config isolation, nil clears one entry,
  corrupt blob reads as absent.

Unit (mocked HTTP):
- Probe status mapping + concurrency + dedup of duplicate candidates +
  empty-credentials short-circuit (iOS: `ConnectionTesterProbeTests`,
  12 cases — port them 1:1).
- `firstReachable`: skips unreachable head, auth-failed counts, order
  decides ties, nothing reachable → null, missing probe entry → not picked.

Integration / manual:
- Acceptance #5 end-to-end against `scripts/sync-stub-server.py`
  (`urls[0]` → dead port, `urls[1]` → stub; verify rotation + persisted
  live URL + no watermark reset).
- Airplane-mode flip, Wi-Fi↔cellular flip, Tailscale toggle.

## 13. Risks & notes

- **Probe cost**: bounded by triggers + debounce; steady state issues zero
  extra requests. Cellular probe of a LAN candidate fails fast (no route)
  or hits the 2 s cap.
- **Doze / background**: triggers fire only while the app (or its sync
  surface) runs; do not schedule background probes for this feature.
- **401-is-reachable** means a profile with a wrong password pins to the
  nearest path and surfaces auth errors from the sync loop — intended.
- **Cleartext policy**: shipping the global cleartext flag mirrors iOS's
  `NSAllowsArbitraryLoads` decision (user-deployed servers on arbitrary
  hosts cannot be allowlisted). Document the Play-review justification the
  same way iOS documents its App-Store one.

## 14. Open questions (answer before/while implementing)

1. Does the Android client have any secondary process or surface (widget,
   IME, Wear, tile) that resolves a server on its own? If yes, it must read
   the live-URL map (never probe) and layer it over shape order, like the
   iOS keyboard/share/intents do; storage must then be cross-process-fresh.
2. What is the Android sync loop's shape (foreground service / polling /
   FCM-driven)? Wire trigger #4 (network-failure re-probe, debounced) into
   its failure path.
3. Does the Android client persist any client-private active-server
   override key that needs the §5.2 promotion migration?
4. Existing Android auto-switch UI inventory — enumerate the screens that
   render `autoSwitchStrategy`/SSID lists so the removal in §9.1 is
   complete.
