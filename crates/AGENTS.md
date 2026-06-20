# PROJECT KNOWLEDGE BASE

**Last refreshed:** 2026-06-20 (auto; 21 workspace crates)

## OVERVIEW

Rust monorepo workspace (root `Cargo.toml`) with strict hexagonal boundaries: library crates in `crates/`, runnable binaries in `apps/` (`uniclip` CLI, `uniclipd` daemon), Tauri packaging in `src-tauri/` (bin entry `src-tauri/src/main.rs` is a 12-line handoff to `uc_tauri::run`). Since ADR-007/008 the background engine is a standalone `uniclipd` daemon (`uc-daemon` + `uc-webserver`); the GUI and CLI are loopback HTTP+WS clients of it.

## STRUCTURE

```text
.                        # repo root = cargo workspace
|- apps/                 # Runnable binaries
|  |- cli/                 # `uniclip` CLI (daemon client; heavy deps feature-gated)
|  |- daemon/              # GUI-agnostic daemon runtime; hosts the `uniclipd` binary
|- crates/               # Library crates (17)
|  # -- Hex core (ADR-005) --
|  |- uc-core/          # Domain models + Port traits only (no external deps)
|  |- uc-application/   # Use cases / orchestrators (depends on uc-core ports only)
|  |- uc-platform/      # OS adapters: clipboard, secure storage, autostart
|  |- uc-app-paths/     # Lightweight directory-layout authority (data/cache/tmp)
|  |- uc-infra/         # Infra adapters: Diesel repos, iroh P2P, encryption, fs, timers
|  |- uc-observability/ # Dual-output tracing, profile filtering, Sentry/analytics scope
|  |- uc-bootstrap/     # Composition root -- the ONLY crate that may depend on core+app+infra+platform at once
|  # -- Daemon split (ADR-007/008) --
|  |- uc-daemon-contract/ # Transport DTOs/contracts shared by client + server
|  |- uc-daemon-process/ # Thin process primitives: PID file, socket path, spawn, health-wait
|  |- uc-daemon-local/  # Local process coordination: auth token, socket discovery, health polling
|  |- uc-webserver/     # Daemon's 127.0.0.1 HTTP + WebSocket API (OpenAPI / ApiEnvelope)
|  |- uc-daemon-client/ # Daemon HTTP + WS client (used by GUI + CLI)
|  # -- Shells / entrypoints --
|  |- uc-desktop/       # Desktop host: runtime, daemon probe, background tasks (GUI-framework-agnostic)
|  |- uc-cli-macros/    # Proc-macros for uc-cli (internal)
|  |- p2p-bench/        # Throwaway perf-spike bins (not shipped; publish = false)
|  # -- Other --
|  |- uc-mobile-proto/  # Pure mobile-sync wire-protocol codec leaf crate (connect-uri)
|  |- uc-mobile/        # UniFFI boundary crate exposing shared Rust to iOS/Android (mobile spike)
|- src-tauri/            # Desktop GUI app (Tauri packaging shell; dir name pinned by tauri-cli)
|  |- src/               # Thin bin: hands off to uc_tauri::run(generate_context!())
|  `- crates/uc-tauri/    # Tauri adapter: commands (via tauri-specta), tray, quick panel, run loop
`- crates/uc-infra/migrations/ # Active infra (diesel) migrations
```


## WHERE TO LOOK

| Task                             | Location                                   | Notes                                                         |
| -------------------------------- | ------------------------------------------ | ------------------------------------------------------------- |
| Tauri run loop & setup           | `src-tauri/crates/uc-tauri/src/run.rs`     | `run()` (line ~200); window/lifecycle, `.manage(...)`, `.setup(...)` |
| IPC command registration         | `src-tauri/crates/uc-tauri/src/specta_builder.rs` | tauri-specta single source of truth (runtime invoke + codegen) |
| Dependency composition           | `crates/uc-bootstrap/src/assembly.rs`      | `wire_dependencies(...)`; GUI-client path via `build_gui_client_context` |
| Runtime/usecase accessors        | `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` | `AppRuntime`, `usecases()` factory                            |
| Tauri commands                   | `src-tauri/crates/uc-tauri/src/commands/`  | Commands call app-layer usecases (or daemon HTTP since ADR-008) |
| Domain contracts (ports)         | `crates/uc-core/src/ports/`                | Add traits here first                                         |
| App workflows                    | `crates/uc-application/src/`               | top-level clipboard_capture / pairing_* / file_transfer / sync_planner; `usecases/` = clipboard_sync, mobile_sync, … |
| Infra implementations            | `crates/uc-infra/src/`                     | Diesel repos, encryption, fs, timers, iroh transport (`network/iroh/`) |
| Platform adapters                | `crates/uc-platform/src/`                  | clipboard (linux X11/Wayland, windows, macos), secure storage, app dirs |
| Daemon API surface               | `crates/uc-webserver/src/api/`             | HTTP + WS endpoints; ApiEnvelope normalization                |
| Legacy reference                 | Removed (2026-02-26)                       | Do not reintroduce legacy module tree                         |

## CODE MAP

| Symbol                     | Type       | Location                                   | Role                               |
| -------------------------- | ---------- | ------------------------------------------ | ---------------------------------- |
| `main`              | fn | `src-tauri/src/main.rs`                 | Process entry; calls `uc_tauri::run`     |
| `run`               | fn | `src-tauri/crates/uc-tauri/src/run.rs`  | Tauri builder + window/run loop          |
| `wire_dependencies` | fn | `crates/uc-bootstrap/src/assembly.rs`   | Hex boundary composition (port→adapter)  |
| `build` (specta)    | fn | `src-tauri/crates/uc-tauri/src/specta_builder.rs` | IPC command registration (single source) |

## CONVENTIONS (PROJECT-SPECIFIC)

- Rust commands run from the repo root (the cargo workspace root); stop if `Cargo.toml` absent.
- Keep `uc-core` pure; no infra/platform dependencies in core.
- New external capability flow: `uc-core/ports` trait -> adapter in `uc-infra` or `uc-platform` -> wire in `uc-bootstrap/src/assembly.rs`.
- Tauri command pattern: command -> `runtime.usecases().x()`; avoid direct `deps` access from command layer.
- Event payloads emitted via `app.emit()` must use `#[serde(rename_all = "camelCase")]`.
- Use `tracing` structured logs; avoid `println!/eprintln!/log` macros in production.
- For iroh/event-loop changes, preserve non-blocking progress; do not block the iroh endpoint while awaiting business stream operations.
- 做产品/架构方向判断前先读根目录 `VISION.md`。

- Daemon HTTP port is deterministic from `UC_PROFILE` via FNV-1a hash (see `uc-daemon-process/src/socket.rs`); no port file exists.
- Daemon auth flow: Bearer file-token → `POST /auth/connect` `{"pid":N,"clientType":"cli"}` → Session JWT; use `Session <jwt>` header afterward.
- `POST /clipboard/dispatch` sends to peers only; dispatched content does NOT appear in sender's `/clipboard/entries` (entries come from OS clipboard captures).

## ANTI-PATTERNS (THIS PROJECT)

- Mixing boundary layers in one change set (`uc-core` + `uc-infra` etc.).
- Adding business logic inside `uc-tauri` command handlers or platform adapters.
- Reintroducing code under any `src-legacy/` path.
- Introducing `unwrap()/expect()` in production paths.
- Emitting snake_case payload fields to frontend events.
- Putting test-only crates in `crates/` as workspace members — use `tests/e2e/` + `[workspace.exclude]` to avoid polluting `cargo check --workspace`.
- Parking RAII guards (e.g. `WorkerGuard`) in library statics + adding host-specific flush/shutdown APIs — init returns the guard; the host shell owns the drop (`process::exit` skips static destructors, losing the buffered tail).
- Shelling out to OS console tools (`kill`/`taskkill`/`tasklist`) for process liveness/termination — use native calls (`libc::kill`, `win_process`); shell-out means fork+exec, locale-dependent output parsing, and console-window flashes from the no-console GUI host. (Existing `lsof`/`netstat` port-lookup fallbacks are the documented exception: locale-stable numeric output, rare path.)
- "Fixing" unix `is_pid_alive` to treat EPERM as alive — `verify_pid_identity` needs EPERM→dead so foreign-user PID reuse reads `Stale`, not `Active` (exe check can't read a foreign process and falls back to Active).

## COMPLEXITY HOTSPOTS

- `crates/uc-bootstrap/src/assembly.rs`: global hex wiring (port→adapter); smallest safe edits only.
- `crates/uc-infra/src/network/iroh/` (`node.rs` ~1.7k lines, `presence_adapter.rs` ~1.5k): iroh endpoint/event-loop internals; preserve non-blocking progress, keep business rules out.
- `crates/uc-platform/src/clipboard/platform/linux/` (X11 + Wayland): most-churned area lately; MIME-alias / self-echo race fixes cluster here.
- `crates/uc-application/src/usecases/mobile_sync/` (`apply_incoming.rs` ~2.1k lines): large, actively-evolving LAN mobile-sync flows.
- `crates/uc-application/src/facade/space_setup/facade.rs` (~2k) + `pairing_inbound/orchestrator.rs` (~1.8k): high-state setup/pairing transitions.
- `crates/uc-core/src/network/`: protocol-critical session / state machines.

## COMMANDS

```bash
# Workspace checks (from the repo root)
cargo check --workspace
cargo test --workspace

# Targeted package quick loop
make check
make build

# E2E tests (from the repo root; requires pre-built binaries)
cargo build -p uc-daemon -p uc-cli
cargo test --manifest-path tests/e2e/Cargo.toml -- --ignored

# Coverage wrapper (from repo root)
bun run test:coverage
```

## NOTES

- `src-legacy/` was removed on 2026-02-26; treat any references as historical context only.
- Root `AGENTS.md` is the navigation index; this file is the Rust-workspace knowledge base covering `crates/`, `apps/`, and `src-tauri/`. Tauri packaging details live in `src-tauri/AGENTS.md`.
- Any change touching `crates/uc-platform/src/clipboard/` (esp. the linux X11/Wayland adapters) should run `cargo test -p uc-platform` before merge. (The network transport is no longer in uc-platform — it lives in `crates/uc-infra/src/network/iroh/`.)
- `uc-mobile` ships on an INDEPENDENT version line (`crates/uc-mobile/Cargo.toml` `version`, not `workspace`) and is released as a `uc-mobile-v*` xcframework via `.github/workflows/build-mobile-core.yml`. The iOS app repo consumes it through SwiftPM `binaryTarget(url:checksum:)`. Full runbook: `docs/packaging/mobile-core-build-release.md`. `uc-mobile-proto` stays on the workspace version (shared with the desktop daemon via `uc-application`).
- Log files live in the platform-conventional log location (separate from the data root since the logs split). Single source of truth: `uc_app_paths::app_log_dir()`. Per-role files `uniclipboard-{gui,daemon,cli}.json.<date>`, daily rotation, 7-day retention (older pruned on start).
- macOS: `~/Library/Logs/app.uniclipboard.desktop[-<profile>]/`
- Linux: `~/.local/state/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows: `%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- Portable ("green") builds keep logs under `<exe>/data/logs/`.
- Older legacy app-data roots may still exist from previous builds, but they are not the current default.
