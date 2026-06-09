# PROJECT KNOWLEDGE BASE

**Last refreshed:** 2026-06-09 (auto; 18 workspace crates)

## OVERVIEW

Tauri v2 desktop backend with strict hexagonal boundaries. The bin entry (`src/main.rs`) is a 12-line handoff to `uc_tauri::run`; domain/application/infra/platform live in workspace crates. Since ADR-007/008 the background engine is a standalone `uniclipd` daemon (`uc-daemon` + `uc-webserver`); the GUI and CLI are loopback HTTP+WS clients of it.

## STRUCTURE

```text
src-tauri/
|- src/                  # Thin bin: hands off to uc_tauri::run(generate_context!())
|- crates/               # Hexagonal workspace (18 crates)
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
|  |- uc-daemon/        # GUI-agnostic daemon runtime; hosts the `uniclipd` binary
|  |- uc-daemon-client/ # Daemon HTTP + WS client (used by GUI + CLI)
|  # -- Shells / entrypoints --
|  |- uc-tauri/         # Tauri adapter: commands (via tauri-specta), tray, quick panel, run loop
|  |- uc-desktop/       # Desktop host: runtime, daemon probe, background tasks (GUI-framework-agnostic)
|  |- uc-cli/           # `uniclip` CLI (daemon client; heavy deps feature-gated)
|  |- uc-cli-macros/    # Proc-macros for uc-cli (internal)
|  `- p2p-bench/        # Throwaway perf-spike bins (not shipped; publish = false)
`- crates/uc-infra/migrations/ # Active infra (diesel) migrations
```


## WHERE TO LOOK

| Task                             | Location                                   | Notes                                                         |
| -------------------------------- | ------------------------------------------ | ------------------------------------------------------------- |
| Tauri run loop & setup           | `crates/uc-tauri/src/run.rs`               | `run()` (line ~200); window/lifecycle, `.manage(...)`, `.setup(...)` |
| IPC command registration         | `crates/uc-tauri/src/specta_builder.rs`    | tauri-specta single source of truth (runtime invoke + codegen) |
| Dependency composition           | `crates/uc-bootstrap/src/assembly.rs`      | `wire_dependencies(...)`; GUI-client path via `build_gui_client_context` |
| Runtime/usecase accessors        | `crates/uc-tauri/src/bootstrap/runtime.rs` | `AppRuntime`, `usecases()` factory                            |
| Tauri commands                   | `crates/uc-tauri/src/commands/`            | Commands call app-layer usecases (or daemon HTTP since ADR-008) |
| Domain contracts (ports)         | `crates/uc-core/src/ports/`                | Add traits here first                                         |
| App workflows                    | `crates/uc-application/src/`               | clipboard_capture / pairing_* / file_transfer / sync_planner  |
| Infra implementations            | `crates/uc-infra/src/`                     | Diesel repos, encryption, fs, timers                          |
| Platform adapters                | `crates/uc-platform/src/`                  | libp2p, clipboard, secure storage                             |
| Daemon API surface               | `crates/uc-webserver/src/api/`             | HTTP + WS endpoints; ApiEnvelope normalization                |
| Legacy reference                 | Removed (2026-02-26)                       | Do not reintroduce legacy module tree                         |

## CODE MAP

| Symbol                     | Type       | Location                                   | Role                               |
| -------------------------- | ---------- | ------------------------------------------ | ---------------------------------- |
| `main`              | fn | `src/main.rs`                           | Process entry; calls `uc_tauri::run`     |
| `run`               | fn | `crates/uc-tauri/src/run.rs`            | Tauri builder + window/run loop          |
| `wire_dependencies` | fn | `crates/uc-bootstrap/src/assembly.rs`   | Hex boundary composition (port→adapter)  |
| `build` (specta)    | fn | `crates/uc-tauri/src/specta_builder.rs` | IPC command registration (single source) |

## CONVENTIONS (PROJECT-SPECIFIC)

- Rust commands run from `src-tauri/` only; stop if `Cargo.toml` absent.
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

## COMPLEXITY HOTSPOTS

- `crates/uc-bootstrap/src/assembly.rs`: global wiring; smallest safe edits only.
- `crates/uc-application/src/`: setup/pairing orchestrators, high-state async transitions.
- `crates/uc-core/src/network/`: protocol-critical state machines.
- `crates/uc-infra/src/network/`: iroh transport internals; keep business rules out.

## COMMANDS

```bash
# Workspace checks (from src-tauri/)
cargo check --workspace
cargo test --workspace

# Targeted package quick loop
make check
make build

# E2E tests (from src-tauri/; requires pre-built binaries)
cargo build -p uc-daemon -p uc-cli
cargo test --manifest-path tests/e2e/Cargo.toml -- --ignored

# Coverage wrapper (from repo root)
bun run test:coverage
```

## NOTES

- `src-legacy/` was removed on 2026-02-26; treat any references as historical context only.
- Current repository root also has parent-level `AGENTS.md`; local file narrows rules to `src-tauri/` workspace details.
- Any change touching `crates/uc-platform/src/adapters/libp2p_network.rs` must run `cargo test -p uc-platform` before merge.
- Current desktop log files live under the app data root's `logs/` directory, using the current app dir name `app.uniclipboard.desktop` plus optional `UC_PROFILE` suffix.
- macOS: `~/Library/Application Support/app.uniclipboard.desktop[-<profile>]/logs/`
- Linux: `~/.local/share/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows: `%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- Older legacy app-data roots may still exist from previous builds, but they are not the current default.
