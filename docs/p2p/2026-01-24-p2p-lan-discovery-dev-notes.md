# P2P LAN mDNS Discovery Dev Notes (2026-01-24)

## Scope

- Worktree: /Users/mark/MyProjects/uniclipboard-desktop/.worktrees/p2p-lan-discovery
- Branch: p2p-lan-discovery
- Goal: mDNS-only LAN discovery + event semantics + wiring

## Core Decisions

- LAN-only discovery: mDNS only, no DHT/relay/hole punching
- No auto-dial; only implicit connect on send/request (not implemented yet)
- NetworkEvent is observational, not a state machine callback
- reachable_peers is best-effort and not a connection guarantee
- NetworkPort remains minimal (no connection lifecycle API)

## Implementation Notes

- Adapter: `Libp2pNetworkAdapter` in `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs`
  - Uses libp2p Swarm + mDNS behaviour
  - Maintains `PeerCaches` for discovered + reachable peers
  - Emits `NetworkEvent::PeerDiscovered`, `PeerLost`, `PeerReady`, `PeerNotReady`
- Swarm startup moved to runtime-ready phase:
  - `wire_dependencies` no longer calls `spawn_swarm`
  - `start_background_tasks` now spawns the swarm inside `tauri::async_runtime`
  - Rationale: avoid tokio reactor panic before runtime exists

## Tests Added

- Unit tests for cache + event mapping helpers
- Real mDNS e2e test (in-process, two swarms)
  - `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`
  - waits for PeerDiscovered both ways with 15s timeout

## Commands Run (Evidence)

- `cargo test -p uc-platform libp2p_network -- --nocapture`
  - 9 tests pass (includes e2e mDNS test)
- `cd src-tauri && cargo test --workspace`
  - Baseline failure: mac_rounded_corners/cocoa unresolved (known issue)
- `bun run tauri:dev`
  - Starts successfully when Keychain access is allowed

## Known Issues / Gotchas

- macOS Keychain prompt:
  - If user cancels, wiring fails with
    `identity store failed: User canceled the operation`
  - Current behavior: wiring panics (intentional strict failure)
- mDNS noise on some interfaces:
  - `libp2p_mdns` may log `No route to host (os error 65)` for virtual interfaces
  - Known harmless warning; already filtered in logging config

## Dependency Notes

- libp2p dependencies moved out of `src-tauri/Cargo.toml` into `uc-platform/Cargo.toml`
- `libp2p-stream` is now used in active platform code (`src-tauri/crates/uc-platform`)
- Tauri log plugin mismatch fix:
  - JS: `@tauri-apps/plugin-log` pinned to 2.7.1 in `package.json`
  - Rust: `tauri-plugin-log` remains 2.x
  - Run `bun install` after version pin change

## Files Touched (Key)

- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs`
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- `src-tauri/crates/uc-platform/Cargo.toml`
- `src-tauri/Cargo.toml`
- `package.json`

## Baseline Failures

- `cd src-tauri && cargo test --workspace` fails due to `mac_rounded_corners/cocoa` missing crate

## Next Steps (if needed)

- If you want dev-friendly behavior, add explicit fallback for Keychain cancel
  (currently strict by design).
