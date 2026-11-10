# Tailcat reconnect probe (reproduction material for upstream)

This directory holds the material behind defect 4 of the [0021 execution plan](../0021-cli-on-demand-service-lifecycle.md). It is not built or tested by this repository.

## Problem

When a paired client process restarts within 0 to 4 s of its previous run, the first dial takes 5.5 to 11 s. After a gap of 6 s or more it takes about 1 s.

The server's magicsock keeps trusting the previous process's direct UDP address for `trustUDPAddrDuration` (6500 ms). The WireGuard handshake response goes to the dead port, and WireGuard retries only every 5 s.

## Files

| File | Purpose |
|---|---|
| `stale-path-reset.patch` | Patch for `github.com/tailscale/tailcat` v0.7.0. The client appends a random 16-byte boot ID to the meow ping. The server resets the path state of that one node when the boot ID changes and a direct path exists. A resend with the same boot ID does not reset. |
| `probe-main.go.txt` | Two-node probe. It dials, exits, and dials again after a chosen gap. |
| `two-node.sh.txt` | Script that runs the probe for gaps of 0 to 8 s. |

The `.txt` suffix keeps the Go and shell sources out of the build and out of lint. Remove it before use.

## Results

- Isolated probe with the patch: every restart connects in about 1 s (original: 5.5 s or more).
- Product topology (three nodes, each one both server and client), same machine:
  - Original: 6 to 17 s, 0 failures.
  - Patch with 1 s meow resend: 2 to 4 s, 4 of 12 runs failed to connect within 40 s.
  - Patch with 250 ms meow resend (change `time.NewTicker(time.Second)` to `250 * time.Millisecond` in `ping`): about 3 s for one peer, 2 of 6 runs failed.

## Open questions

- Why the first meow pings do not reach the server for about 2 s in the product topology (the probe needs less than 0.5 s). This roughly matches the 3 s "UDP is blocked" netcheck on the test machine, which has a proxy interface.
- Why some runs never connect with the patch.
- No cross-machine LAN test was done.

## Side finding

The tailcat server never evicts entries from `b.clients`. A new client key per process leaks about two goroutines and 40 KB per key. For this reason a per-process client key was rejected.
