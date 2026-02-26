# UC-APP USECASES

Follow parent rules in `AGENTS.md` and `crates/uc-app/AGENTS.md`.
This directory owns application decisions and orchestration.

## OVERVIEW

For cross-device clipboard transfer, usecases are the decision layer.
They decide:

- when to send clipboard payloads,
- when to apply remote payloads,
- how to avoid self-echo loops,
- which ports are invoked in what order.

They must not embed transport/storage implementation details.

## WHERE TO LOOK

- Clipboard read/restore flows:
  - `crates/uc-app/src/usecases/internal/capture_clipboard.rs`
  - `crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs`
- Network and pairing coordination:
  - `crates/uc-app/src/usecases/pairing/`
  - `crates/uc-app/src/usecases/start_network_after_unlock.rs`
- Lifecycle startup ordering:
  - `crates/uc-app/src/usecases/app_lifecycle/mod.rs`

## CONVENTIONS

- Use `uc-core` ports only. No concrete `uc-infra` or `uc-platform` types in usecases.
- Keep each usecase single-intent and observable with structured `tracing` spans.
- For remote clipboard apply path, always respect `ClipboardChangeOrigin` contract to prevent re-capture loops.
- For outbound clipboard sync, do serialization/encryption decisions in usecase layer, transport send via `NetworkPort`.
- Protocol-level literals (MIME names, protocol tags, field identifiers) must come from shared `uc-core` constants/types, not duplicated string literals.
- If command-side workflow needs "read current system state then execute", expose a dedicated usecase entrypoint instead of building flow in command handlers.
- Propagate errors with context; no silent `Ok(())` fallbacks for failed sync actions.
- In outbound sync fanout, keep stage-level failure semantics explicit (ensure/connect vs payload send); do not collapse into ambiguous "network failed".
- Retry/backoff policy belongs to usecase decisions, not platform adapters.

## ANTI-PATTERNS

- Calling adapter-specific methods from usecases.
- Mixing pairing policy changes with clipboard materialization refactors in one diff.
- Bypassing `RestoreClipboardSelectionUseCase` and writing directly to system clipboard from non-clipboard usecases.
- Logging decrypted payload content or secrets.

## HIGH-RISK FILES

- `crates/uc-app/src/usecases/internal/capture_clipboard.rs`
- `crates/uc-app/src/usecases/clipboard/restore_clipboard_selection.rs`
- `crates/uc-app/src/usecases/app_lifecycle/mod.rs`
- `crates/uc-app/src/usecases/pairing/orchestrator.rs`

## CHECKLIST FOR CROSS-DEVICE CLIPBOARD CHANGES

1. Outbound path and inbound path are separated (no hidden coupling).
2. Dedupe key strategy is explicit (event id / source device / snapshot or content hash).
3. `ClipboardChangeOrigin` is set/consumed correctly around restore/apply paths.
4. Tests cover happy path, duplicate message, and channel/error path.
5. Business policy remains in usecases; wiring/adapters stay mechanical.
6. New/changed usecase entrypoints have focused tests (including command-facing entry methods when added).
7. Include cold-start connectivity coverage: peer discovered but not yet connected, ensure/send still converge correctly.

## COMMANDS

```bash
# from src-tauri/
cargo check -p uc-app
cargo test -p uc-app
```
