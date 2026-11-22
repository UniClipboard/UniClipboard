# Desktop network settings: pinned Tailcat capability review

- Date: 2026-11-22.
- Repository baseline: `fcf5611662039c1db79fc6537c4f79aff2a5b436`.
- Scope: Read-only group F follow-up to the [settings work agreement](desktop-settings-parity.md).
- Reviewed dependency: `github.com/tailscale/tailcat v0.7.0` and `tailscale.com v1.103.0-pre.0.20260916030321-a2263542f260`, as pinned in `go.mod`.
- Local pinned `tailcat.go` SHA-256: `78e5678e383b743233f988ae4c706645798ad06bb552d2b60e32c8937c9c5867`.
- Status: Capability review complete. No product code, saved settings, public contract or dependency changed. No probe, content transfer, GUI, browser or VM ran.

This review describes the pinned source only. It does not assert that v0.7.0 is
the latest upstream release or that any public relay is currently reachable.
Public API presence is source evidence, not runtime acceptance.

## Existing assembly and configuration

`app/service/internal/host/node.go:87` creates `tailcat.New()` explicitly.
The built-in `engine.PluginInstance` currently supplies the plugin and
`Builtin: true`, but no plugin `Config`. The built-in has no plugin-specific
Schema. `internal/plugins/tailcat/tailcat.go:46` accepts but ignores its
`plugin.Config` argument. It calls `tcnet.New` with only the private directory
and a discarded network logger. Generic TOML values under
`plugins.instances."builtin.tailcat".config` therefore do not configure the
current transport merely because the container can store them.

`internal/tcnet/tcnet.go:49` loads persistent client/server keys and PSK, creates
one `tailcat.Server`, and restores its `Region` from the previously persisted
address. `Dial` at line 128 creates clients with only `Server`, `Key` and `Logf`.
The normal backend is one reserved built-in instance; current alias support is
for external Go backends, not additional aliases of `builtin.tailcat`.

Do not add Desktop-global transport fields or save controls that the backend
does not consume. Maintain the boundary: Desktop renders; the service owns
configuration; the built-in plugin owns its network transport; Engine owns
device trust and instance authorizations.

## Capability table

| v1 setting or operation | Maintained public surface in the pinned dependency | Current v2 support and direct implementation verdict | Apply/restart boundary | Secret boundary |
|---|---|---|---|---|
| LAN-only / disable relay fallback | No public LAN-only, no-DERP, interface-binding or endpoint-policy field on `tailcat.Server` or `tailcat.Client`. Server startup explicitly selects a DERP bootstrap region; client `Ping`/dial registers through DERP. | Not directly implementable with this API. `Server.AllowProxy` controls tunneled proxy destinations, not the physical path. An empty DERP map is a broken bootstrap, not a supported LAN-only mode. | No truthful apply rule exists yet. Requires a maintained upstream capability or an explicitly designed backend alternative, followed by policy E2E. | A control must not print peer addresses, candidate IPs or node keys. Turning off content relay must not imply that telemetry, updates or pairing rendezvous stop. |
| Allow overlay/VPN interface addresses | No public physical endpoint inclusion/exclusion callback on the Server/Client API. Tailcat owns `netmon`, magicsock and endpoint advertisement inside its private backend. | Not directly implementable. v1's iroh policy for CGNAT/ULA addresses does not map to a Tailcat field. The tunnel's virtual ULA is not a physical VPN candidate toggle. | Requires an upstream path-policy API; no fake saved preference or private-field mutation. | Physical interface addresses stay inside the transport. Return only finite support/effective-state categories. |
| Built-in DERP source and selected region | `DefaultDERPMapURL`, `FetchDERPMap`, `Server.Region`, `Server.RegionID`, `Server.DERPMapURL`, `Server.DERPMapCache`. | Public configuration APIs exist. Current `tcnet.New` uses the default source for first startup and then pins the embedded region. A static explanation of this behavior needs no transport change. A live region list/selection needs service DTOs and actual configuration wiring. | Set server fields before first `Start`; no public region hot-update API. Preserve current endpoint pinning until an address-change policy is accepted. | The default map URL is public. Do not expose the persisted Tailcat address to display its embedded region; that address includes a PSK. |
| Custom DERP map or one explicit custom region | `Server.Region` accepts `*tailcfg.DERPRegion`; `Server.DERPMapURL` selects a map when Region is nil. `Client.DERPMapURL` resolves addresses that lack embedded details. | Representable by maintained APIs, but not currently wired. A single region with multiple nodes is supported; the address format currently embeds at most one region. Merely changing Client.DERPMapURL does not override the relay embedded in a peer address. | Requires backend initialization changes and service restart. Existing server address pinning overrides a map-source change. Replacing the selected region changes the secret transport address; decide old-peer behavior first. | Keep map URLs credential-free and redact query strings when returning diagnostics. Custom hostnames/IPs may reveal private infrastructure; expose intentionally selected display fields only. Never enable `InsecureForTests` in production. |
| Custom relay username/password/token | No dedicated relay HTTP authentication provider, custom header or per-relay secret hook is exposed by Server/Client. `tailcfg.DERPNode` and Tailcat's `wireNode` do not carry application authorization credentials. | Not directly implementable safely through the supported API. A DERP-map fetch is a different request from the DERP tunnel connection. Do not assume map authentication authenticates the relay. Do not mutate process-wide `http.DefaultClient` or put credentials into a URL. | Requires a maintained upstream secret/authentication API and accepted storage/apply semantics before implementation. | Use write-only private plugin settings with presence indicators if support is later added. Do not put tokens into peer tickets, ordinary TOML exports, URLs, logs or errors. Existing WireGuard PSK/node identity is not a custom relay login. |
| Explicit connectivity/latency probe | `FetchDERPMap(ctx, opts...)`; `PickBestRegion(ctx, dm)`; `Client.Ping(ctx)` returning `PingResult.Latency`; `Client.DiscoPing(ctx)`. | Upstream primitives are sufficient for an explicitly requested active probe, but v2 has no public service operation exposing them. `PickBestRegion` returns only the selected ID, not the full per-region latency report. Ping measures the DERP handshake; it does not prove a content transfer. | A standalone explicit probe needs no service restart, but may start a client/UDP sockets and alter direct-path discovery. It needs a bounded service operation and cancellation/cleanup semantics first. | Never return raw PingResult endpoints, keys, address strings or raw library errors. Probe only an authorized peer or an explicitly selected relay; provide finite verdicts and duration. |
| Congestion control: CUBIC / BBR3 | No public congestion-controller field/setter in Tailcat. Unic sync uses TCP port 7777 inside the WireGuard/netstack tunnel, not v1's iroh QUIC configuration. | Not directly implementable. The v1 selector cannot be connected to the current transport by renaming the field. Do not reach into private gVisor state or add a second transport solely to save a setting. | No supported apply/restart rule until a maintained API or a reviewed transport decision exists. | Report finite selected/supported values only if the transport can actually enforce them. Do not infer algorithm support from indirect dependency names. |

## Pinned source map

Upstream references below identify the exact reviewed version. The local module
cache was used for inspection; no upstream endpoint was contacted by this task.

- [Server public fields](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L408-L549): identity, Region, RegionID, map source/cache, allowed clients, TCP/UDP proxy/listener controls. The latter do not control physical-network candidates.
- [Server startup](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L567-L653): resolves the bootstrap region and builds a private DERP map before creating the transport.
- [Client public fields](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L1823-L1859): address, identity, logger and map source/cache; configuration is required before first use.
- [Tailcat address](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L987-L993): embeds the full region. [ConnInfo](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L148-L188) also contains the PSK and permits at most one embedded region.
- [Map fetch API](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L1181-L1210) and [fetch implementation](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L1265-L1309): ten-second fetch bound, process-wide default HTTP client, conditional cache requests and stale-map fallback. A fetch can return a cached/stale map; a returned map does not prove live relay availability.
- [Region selection](https://github.com/tailscale/tailcat/blob/v0.7.0/pickregion.go#L21-L63): active netcheck, STUN and fallback probing; returns the best region ID.
- [Ping](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L2060-L2098) and [DiscoPing](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L2145-L2195): explicit traffic; DiscoPing can advertise endpoints and upgrade the connection.
- [Server status](https://github.com/tailscale/tailcat/blob/v0.7.0/tailcat.go#L2496-L2501): passive server snapshot. The pinned Client has no equivalent public passive status method. Do not transfer a claim about a later branch's Client API to this pin.
- [DERP wire node](https://github.com/tailscale/tailcat/blob/v0.7.0/wire.go#L43-L64): hostname, certificate name, IP addresses and ports, but no application relay authorization headers.

Repository integration references:

- `internal/plugins/tailcat/tailcat.go:40`: built-in Spec has no settings digest/Schema. Line 46: ignored Config argument.
- `internal/tcnet/tcnet.go:49`: persistent initialization and pinning. Line 88: Start publishes/persists the address. Line 128: client construction.
- `internal/tcnet/path.go:25`: passive incoming-path reduction; outgoing remains unknown. It protects before-start/concurrent-close status access and discards raw identity/endpoint fields.
- `internal/tcnet/credentials.go:24`: private server/client keys, PSK and complete secret address, atomically persisted with restricted permissions.
- `internal/tcnet/ephemeral.go:32` and `:86`: one-shot transfer has separate fresh Server/Client construction. A policy added only to the persistent backend does not cover temporary transfer automatically.
- `app/service/internal/host/node.go:87` and `:130`: explicit built-in construction without transport config. `app/service/internal/host/plugin_settings.go:79` resolves settings through installed external manifests/Schemas; it does not supply a built-in Tailcat Schema automatically.
- `engine/plugins.go:150` and `internal/store/backend_instances.go:8`: Engine binds an instance's serialized Config digest before initialization. A changed digest atomically removes that instance's device bindings and external channels while preserving device trust and other instances.
- `internal/localconfig/manager.go:95`: existing restart-key reporting. New transport fields need an explicit restart marker/application rule; merely placing a value in the generic Config map does not establish one.

## Work that can proceed without transport changes

1. Explain the current network capability limits in the UI instead of showing saveable no-op controls. Reuse existing `SyncBackends` instance running state for that status; do not label it as direct/DERP health.
2. Preserve the existing Tailcat content-policy settings. Text/image/file permission and automatic file threshold already apply through Engine policy and are independent of transport routing.
3. Prepare E2E scenarios and service-operation proposals for explicit probes and public relay selection. Source-level representability does not authorize adding those public operations.

There is no currently wired transport preference among LAN-only, overlay,
custom relay authentication or congestion control that can be completed solely
by adding a settings control. Public DERP initialization fields reduce the
required upstream work, but still need local runtime and authorization design.

## Decisions required before implementation

1. **Relay address changes.** Choose whether editing a pinned region requires
   explicit re-pairing/reapproval, or a designed authenticated address-update
   flow. Existing tickets and peer paths contain the previous address. Do not
   silently regenerate keys, overwrite trusted paths or bypass approval.
2. **Config identity and authorization.** Passing newly wired values through
   `Engine.PluginInstance.Config` invokes the existing config-digest boundary.
   Retaining that rule means a changed backend config invalidates that
   instance's old bindings before restart. Any exception for display-only or
   routing-only fields changes the [backend identity boundary](../contracts/backend-migration-contract.md#implementation-and-instance-identity)
   and needs the user's decision; it is not a UI implementation detail.
3. **Public settings and probe operations.** Specify typed values, validation,
   restart state, sanitized status, probe authorization, budgets and cleanup
   before changing the [plugin settings contract](../contracts/plugin-settings-contract.md)
   or [sync backend contract](../contracts/sync-backend-contract.md). Active
   probes must be explicit and separate from the [passive observability collector](../contracts/observability-contract.md).
4. **LAN-only meaning and coverage.** v1's control disabled relay fallback;
   it was not a total network kill switch. Define v2's bootstrap, physical-path,
   persistent/temporary-transfer and external-module coverage before selecting
   an upstream API. Copying v1's iroh/QUIC implementation is not a Tailcat fix.
5. **Unsupported transport controls.** Decide whether LAN-only, overlay policy,
   relay credentials and congestion selection stay unavailable pending
   maintained upstream support, or receive a separate reviewed backend design.
   This review does not authorize vendoring, reflection, unsafe access or a
   dependency upgrade.

## Verification boundary

This task read repository and pinned module source, inspected the v1 settings
implementation, and recomputed the pinned `tailcat.go` digest. It did not run
network E2E, fetch a DERP map, call Ping/DiscoPing/netcheck, query private
credentials, or evaluate live relay health. Runtime and cross-platform results
remain NOT_RUN. Run `go run ./tools/docscheck` to validate document links; that
command does not validate the proposed network behaviors.
