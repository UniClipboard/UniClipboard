# Tailcat Client status dependency

- State: BLOCKED_UPSTREAM.
- Checked: 2026-11-19 UTC.
- Dependency: `github.com/tailscale/tailcat v0.7.0` in `go.mod`.
- Scope: passive status for outgoing Tailcat sessions.

The pinned dependency provides `Server.Status()`. The pinned Client API and the
upstream main source expose no equivalent passive Client status method. The
incoming path continues to use the Server snapshot. Outgoing path samples stay
`unknown`. Treat them as unsupported coverage when computing known path shares.

`Client.DERPRegion()` reports the configured relay region. It does not tell
whether current traffic uses that relay. `Client.Ping` and `Client.DiscoPing` generate
traffic, and the latter can prompt direct discovery. Using them would change the
population being measured.

Resume when a maintained release exposes a read-only Client snapshot. Check
before-start, concurrent close and stale-peer behavior. Pin the released revision.
Repeat the forced-relay and relay-removal tests. Require independently verified
outgoing direct and DERP samples without sampler-generated traffic.

Sources: [latest release](https://github.com/tailscale/tailcat/releases/tag/v0.7.0),
[pinned Client API](https://pkg.go.dev/github.com/tailscale/tailcat@v0.7.0#Client),
[upstream source](https://github.com/tailscale/tailcat/blob/main/tailcat.go),
[local real-stack receipt](0020-tailcat-sync-observability.md).
