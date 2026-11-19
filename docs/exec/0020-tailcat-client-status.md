# Tailcat Client status dependency

- State: BLOCKED_UPSTREAM.
- Checked: 2026-11-19 UTC.
- Dependency: `github.com/tailscale/tailcat v0.7.0` in `go.mod`.
- Scope: passive status for outgoing Tailcat sessions.

## Recheck evidence

The official release list still labels v0.7.0 as Latest. The official main
`tailcat.go` and the cached, checksum-verified v0.7.0 module expose
`Server.Status()` without a corresponding passive Client snapshot. The main
changelog's unreleased changes do not add that interface. This recheck does
not accept a cached commits page as an immutable main revision.

`go mod verify` passed on 2026-11-19. The local pinned `tailcat.go` SHA-256 is
`78e5678e383b743233f988ae4c706645798ad06bb552d2b60e32c8937c9c5867`.
No dependency update, private-field access, active probe or runtime route
reclassification was performed.

The pinned dependency provides `Server.Status()`. The pinned Client API and the
upstream main source expose no equivalent passive Client status method. The
incoming path continues to use the Server snapshot. Outgoing path samples stay
`unknown`. Treat them as unsupported coverage when computing known path shares.

On checked main, `Client.DERPRegion()` reports the configured relay region. It does not tell
whether current traffic uses that relay. `Client.Ping` and `Client.DiscoPing` generate
traffic, and the latter can prompt direct discovery. Using them would change the
population being measured.

Resume when a maintained release exposes a read-only Client snapshot. Check
before-start, concurrent close and stale-peer behavior. Pin the released revision.
Repeat the forced-relay and relay-removal tests. Require independently verified
outgoing direct and DERP samples without sampler-generated traffic.

Sources: [latest release](https://github.com/tailscale/tailcat/releases/tag/v0.7.0),
[official release list](https://github.com/tailscale/tailcat/releases),
[pinned Client API](https://pkg.go.dev/github.com/tailscale/tailcat@v0.7.0#Client),
[upstream source](https://github.com/tailscale/tailcat/blob/main/tailcat.go),
[upstream changelog](https://github.com/tailscale/tailcat/blob/main/CHANGELOG.md),
[local real-stack receipt](0020-tailcat-sync-observability.md).
