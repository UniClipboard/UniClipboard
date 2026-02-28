# UC-TAURI BOOTSTRAP

Follow parent rules in `AGENTS.md` and `crates/uc-tauri/AGENTS.md`.
This directory is the composition and runtime-bridge layer.

## OVERVIEW

`bootstrap/` wires concrete adapters into `AppDeps` and hosts runtime background loops.
For cross-device clipboard sync, this layer should only:

- start/stop loops,
- subscribe to port streams,
- forward events into app-layer use cases,
- emit frontend events with stable payload contracts.

It must not own business decisions.

## WHERE TO LOOK

- Dependency wiring and background startup: `crates/uc-tauri/src/bootstrap/wiring.rs`
- Clipboard callback entrypoint: `crates/uc-tauri/src/bootstrap/runtime.rs`
- Existing background loop patterns:
  - `run_pairing_event_loop` in `crates/uc-tauri/src/bootstrap/wiring.rs`
  - `run_pairing_action_loop` in `crates/uc-tauri/src/bootstrap/wiring.rs`

## CONVENTIONS

- Keep this directory as orchestration glue only.
- For clipboard sync, subscribe via focused ports (for example `ClipboardTransportPort::subscribe_clipboard`) and hand off to `uc-app` use cases.
- Do not parse domain policy in bootstrap. Domain decisions live in `uc-app` and `uc-core`.
- Do not perform direct repository mutations from bootstrap loops.
- Keep usecase constructor wiring in sync with `uc-app` signatures; when constructor params change, update bootstrap accessor wiring immediately.
- Keep event payload compatibility strict:
  - any `app.emit(...)` payload struct must use `#[serde(rename_all = "camelCase")]`.
- Use structured `tracing` and never log secrets or raw decrypted clipboard payload bytes.

## ANTI-PATTERNS

- Implementing sync policy directly in `wiring.rs` (retry policy, conflict policy, dedupe policy).
- Writing to system clipboard directly from bootstrap loops.
- Calling `runtime.deps` internals from command handlers instead of usecase accessors.
- Mixing setup/pairing refactors into clipboard-sync changes.

## HIGH-RISK FILES

- `crates/uc-tauri/src/bootstrap/wiring.rs`
- `crates/uc-tauri/src/bootstrap/runtime.rs`

## CHECKLIST FOR CLIPBOARD SYNC LOOP CHANGES

1. New loop is started from `start_background_tasks` and isolated from pairing loops.
2. Loop exits cleanly on channel close and logs observable errors.
3. Clipboard apply path goes through app-layer use case, not direct adapter calls.
4. `ClipboardChangeOrigin` safeguards are respected to avoid echo loops.
5. Added tests cover successful forwarding and channel/error shutdown behavior.

## CHECKLIST FOR USECASE WIRING CHANGES

1. `UseCases::*` accessor constructor args exactly match current `uc-app` usecase signature.
2. Command handlers affected by wiring changes still call accessors only (no fallback to `runtime.deps` business IO).
3. Integration tests that construct the changed usecase are updated in the same diff.
4. Verification includes `cargo check -p uc-tauri` and relevant `cargo test -p uc-app`/`uc-tauri` paths.

## COMMANDS

```bash
# from src-tauri/
cargo check -p uc-tauri
cargo test -p uc-tauri

# optional focused loop tests
cargo test -p uc-tauri pairing_action_loop
cargo test -p uc-tauri pairing_event_loop
```
