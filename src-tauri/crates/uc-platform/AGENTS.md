# UC-PLATFORM

Follow parent rules in `AGENTS.md`. This crate is OS/runtime adapter territory.

## OVERVIEW

`uc-platform` provides concrete platform integrations: clipboard, secure storage, identity store, runtime event loops, and libp2p networking adapters.

## WHERE TO LOOK

- libp2p adapter: `crates/uc-platform/src/adapters/libp2p_network.rs`.
- Clipboard platform implementations: `crates/uc-platform/src/clipboard/platform/`.
- Runtime/event bus: `crates/uc-platform/src/runtime/`.
- Secure storage + identity: `crates/uc-platform/src/system_secure_storage.rs`, `crates/uc-platform/src/identity_store.rs`.

## CONVENTIONS

- Keep business decisions out of adapters; convert external signals to port-level semantics only.
- Platform-specific branches must stay localized (macOS/windows/linux files).
- Respect port behavior contracts from `uc-core` and wiring expectations from `uc-tauri`.
- Prefer explicit error propagation + structured `tracing` context.
- Keep swarm/event-loop drivers responsive: command handling must not block polling progress.
- Business stream operations should run through cloned control handles; avoid long-running waits that hold `&mut Swarm`.
- Use layered timeout budgets: outer command timeout > inner operation budgets (`open + write + close`) with explicit buffer.
- For command/stream diagnostics, log structured command lifecycle fields (`cmd_id`, `op`, `peer_id`, `elapsed_ms`).

## ANTI-PATTERNS

- Embedding usecase or orchestration logic in adapter implementations.
- Cross-calling tauri APIs directly from platform internals.
- Changing protocol framing behavior without pairing-flow verification.
- Treating dropped result receivers as root cause without checking upstream poll-loop starvation or scheduling issues.

## HIGH-RISK FILES

- `crates/uc-platform/src/adapters/libp2p_network.rs`
- `crates/uc-platform/src/adapters/pairing_stream/service.rs`
- `crates/uc-platform/src/runtime/runtime.rs`

## COMMANDS

```bash
# from src-tauri/
cargo check -p uc-platform
cargo test -p uc-platform
```
