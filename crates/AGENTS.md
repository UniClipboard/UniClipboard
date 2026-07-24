# PROJECT KNOWLEDGE BASE

**Last refreshed:** 2026-07-24 (auto; 16 workspace crates)

## OVERVIEW

Desktop Rust workspace (root `Cargo.toml`): system adapters and daemon libraries live in `crates/`, runnable `uniclip` and `uniclipd` binaries live in `apps/`, and Tauri packaging lives in `src-tauri/`. The portable engine is owned by the independent `UniClipboard/core` repository and is consumed here through one immutable Git revision. The GUI and CLI are loopback HTTP+WS clients of the standalone daemon.

## STRUCTURE

```text
.                        # repo root = cargo workspace
|- apps/                 # Runnable binaries
|  |- cli/                 # `uniclip` CLI (daemon client; heavy deps feature-gated)
|  |- daemon/              # GUI-agnostic daemon runtime; hosts the `uniclipd` binary
|- crates/               # Library crates (12)
|  # -- Desktop host adapters --
|  |- uc-platform/      # OS adapters: clipboard, secure storage, autostart
|  |- uc-app-paths/     # Lightweight directory-layout authority (data/cache/tmp)
|  |- uc-observability/ # Dual-output tracing, profile filtering, Sentry/analytics scope
|  |- uc-bootstrap/     # Desktop host capability preparation for the independent core engine
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
|- src-tauri/            # Desktop GUI app (Tauri packaging shell; dir name pinned by tauri-cli)
|  |- src/               # Thin bin: hands off to uc_tauri::run(generate_context!())
|  `- crates/uc-tauri/    # Tauri adapter: commands (via tauri-specta), tray, quick panel, run loop
```

## WHERE TO LOOK

| Task                      | Location                                             | Notes                                                                   |
| ------------------------- | ---------------------------------------------------- | ----------------------------------------------------------------------- |
| Tauri run loop & setup    | `src-tauri/crates/uc-tauri/src/run.rs`               | `run()` (line ~200); window/lifecycle, `.manage(...)`, `.setup(...)`    |
| IPC command registration  | `src-tauri/crates/uc-tauri/src/specta_builder.rs`    | tauri-specta single source of truth (runtime invoke + codegen)          |
| Core revision             | `Cargo.toml`                                         | One immutable `UniClipboard/core` revision for all consumers            |
| Desktop host preparation  | `crates/uc-bootstrap/src/wiring/`                    | Desktop paths, secure storage and clipboard selection                   |
| Runtime/usecase accessors | `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` | `AppRuntime`, `usecases()` factory                                      |
| Tauri commands            | `src-tauri/crates/uc-tauri/src/commands/`            | Commands call app-layer usecases (or daemon HTTP since ADR-008)         |
| Platform adapters         | `crates/uc-platform/src/`                            | clipboard (linux X11/Wayland, windows, macos), secure storage, app dirs |
| Daemon API surface        | `crates/uc-webserver/src/api/`                       | HTTP + WS endpoints; ApiEnvelope normalization                          |
| Legacy reference          | Removed (2026-02-26)                                 | Do not reintroduce legacy module tree                                   |

## CODE MAP

| Symbol           | Type | Location                                          | Role                                     |
| ---------------- | ---- | ------------------------------------------------- | ---------------------------------------- |
| `main`           | fn   | `src-tauri/src/main.rs`                           | Process entry; calls `uc_tauri::run`     |
| `run`            | fn   | `src-tauri/crates/uc-tauri/src/run.rs`            | Tauri builder + window/run loop          |
| `build` (specta) | fn   | `src-tauri/crates/uc-tauri/src/specta_builder.rs` | IPC command registration (single source) |

## CONVENTIONS (PROJECT-SPECIFIC)

- Rust commands run from the repo root (the cargo workspace root); stop if `Cargo.toml` absent.
- Portable engine, protocol, persistence, migration, and binding changes belong in `UniClipboard/core`; never recreate those packages here.
- Upgrade the engine only by changing the single pinned revision in root `Cargo.toml` and updating `Cargo.lock`.
- Desktop-only capability flow: platform adapter -> `uc-bootstrap/src/wiring/` -> `HostCapabilities` -> `Engine::start`.
- Tauri command pattern: command -> `runtime.usecases().x()`; avoid direct `deps` access from command layer.
- Event payloads emitted via `app.emit()` must use `#[serde(rename_all = "camelCase")]`.
- Use `tracing` structured logs; avoid `println!/eprintln!/log` macros in production.
- 做产品/架构方向判断前先读根目录 `VISION.md`。

- Daemon HTTP port is deterministic from `UC_PROFILE` via FNV-1a hash (see `uc-daemon-process/src/socket.rs`); no port file exists.
- Daemon auth flow: Bearer file-token → `POST /auth/connect` `{"pid":N,"clientType":"cli"}` → Session JWT; use `Session <jwt>` header afterward.
- `POST /clipboard/dispatch` sends to peers only; dispatched content does NOT appear in sender's `/clipboard/entries` (entries come from OS clipboard captures).

## ANTI-PATTERNS (THIS PROJECT)

- Copying core source, migrations, bindings, or LAN protocol packages back into desktop.
- Adding a local path, branch, or floating tag for any package owned by `UniClipboard/core`.
- Depending on core implementation packages from desktop production code instead of `uc-engine`.
- Adding business logic inside `uc-tauri` command handlers or platform adapters.
- Reintroducing code under any `src-legacy/` path.
- Introducing `unwrap()/expect()` in production paths.
- Emitting snake_case payload fields to frontend events.
- Putting test-only crates in `crates/` as workspace members — use `tests/e2e/` + `[workspace.exclude]` to avoid polluting `cargo check --workspace`.
- Parking RAII guards (e.g. `WorkerGuard`) in library statics + adding host-specific flush/shutdown APIs — init returns the guard; the host shell owns the drop (`process::exit` skips static destructors, losing the buffered tail).
- Shelling out to OS console tools (`kill`/`taskkill`/`tasklist`) for process liveness/termination — use native calls (`libc::kill`, `win_process`); shell-out means fork+exec, locale-dependent output parsing, and console-window flashes from the no-console GUI host. (Existing `lsof`/`netstat` port-lookup fallbacks are the documented exception: locale-stable numeric output, rare path.)
- "Fixing" unix `is_pid_alive` to treat EPERM as alive — `verify_pid_identity` needs EPERM→dead so foreign-user PID reuse reads `Stale`, not `Active` (exe check can't read a foreign process and falls back to Active).

## COMPLEXITY HOTSPOTS

- `crates/uc-platform/src/clipboard/platform/linux/` (X11 + Wayland): most-churned area lately; MIME-alias / self-echo race fixes cluster here.
- `apps/daemon/src/daemon/mobile_lan_lifecycle.rs`: explicit LAN compatibility lifecycle; it must never react to P2P failure.
- `crates/uc-webserver/src/`: daemon HTTP/WS API plus the explicitly enabled LAN compatibility surface.

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
- Any change touching `crates/uc-platform/src/clipboard/` (especially the Linux X11/Wayland adapters) should run the package's focused validation before merge.
- Core and LAN compatibility releases are produced only by `UniClipboard/core`; desktop keeps no mobile binding source or release workflow.
- Log files live in the platform-conventional log location (separate from the data root since the logs split). Single source of truth: `uc_app_paths::app_log_dir()`. Per-role files `uniclipboard-{gui,daemon,cli}.json.<date>`, daily rotation, 7-day retention (older pruned on start).
- macOS: `~/Library/Logs/app.uniclipboard.desktop[-<profile>]/`
- Linux: `~/.local/state/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows: `%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- Portable ("green") builds keep logs under `<exe>/data/logs/`.
- Older legacy app-data roots may still exist from previous builds, but they are not the current default.
