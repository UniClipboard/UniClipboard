# PROJECT KNOWLEDGE BASE

**最后刷新：** 2026-10-07（自动；16 个工作区 crate）

## OVERVIEW

桌面 Rust 工作区以根目录 `Cargo.toml` 为入口：系统适配器和守护进程库位于 `crates/`，`uniclip` 与 `uniclipd` 位于 `apps/`，桌面 GUI 宿主（Go/Wails，唯一的桌面宿主）位于 `apps/gui-go/`，其共享 React 前端源码位于 `apps/gui/src`。可移植引擎由独立的 `UniClipboard/Engine` 仓库拥有，本仓通过一个固定发布标签使用它。GUI 和 CLI 都通过本机 HTTP 与 WebSocket 访问独立守护进程。

## STRUCTURE

```text
.                        # repo root = cargo workspace
|- apps/                 # Runnable binaries
|  |- daemon/              # GUI-agnostic daemon runtime; hosts the `uniclipd` binary
|  |- quick-panel/         # GPUI quick panel app (`uniclip-quick-panel`, macOS default)
|- tools/                # Development-only crates (never in production builds)
|  |- uc-dev-cli/          # `uc-dev-cli` development and diagnostics CLI (user-facing `uniclip` is apps/cli-go)
|- crates/               # Library crates (13)
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
|  |- uc-cli-macros/    # Proc-macros for uc-dev-cli (internal)
|  |- p2p-bench/        # Throwaway perf-spike bins (not shipped; publish = false)
|  # -- Other --
|  |- quick-panel-core/ # Platform-independent logic of the GPUI quick panel: query model, state machine, ports
```


## WHERE TO LOOK

| Task                      | Location                                   | Notes                                                                   |
| ------------------------- | ------------------------------------------ | ----------------------------------------------------------------------- |
| Desktop host (Go/Wails)   | `apps/gui-go/`                             | Window/tray/updater/packaging; talks to the daemon over loopback HTTP/WS |
| Host command contract     | `apps/gui-go/*.go` (`HostService`), `docs/architecture/gui-go-host-commands.md` | Go methods are the contract; Wails generates the TS bindings |
| Engine 发布版本           | `Cargo.toml`                               | 所有使用方共享一个固定的 `UniClipboard/Engine` 发布标签                 |
| Desktop host preparation  | `crates/uc-bootstrap/src/wiring/`          | Desktop paths, secure storage and clipboard selection                   |
| Desktop runtime           | `crates/uc-desktop/src/runtime.rs`         | Framework-agnostic desktop runtime shared by host shells                |
| Quick panel (GPUI)        | `apps/quick-panel/`, `crates/quick-panel-core/` | Native macOS quick panel hosted by the Go shell                    |
| Platform adapters         | `crates/uc-platform/src/`                  | clipboard (linux X11/Wayland, windows, macos), secure storage, app dirs |
| Daemon API surface        | `crates/uc-webserver/src/api/`             | HTTP + WS endpoints; ApiEnvelope normalization                          |
| Legacy reference          | Removed (2026-02-26)                       | Do not reintroduce legacy module tree                                   |

## CODE MAP

The desktop host is Go (`apps/gui-go`); the Rust binaries are `apps/daemon` (`uniclipd`) and `apps/quick-panel`. There is no Rust GUI entry point in the workspace besides the quick panel.


## CONVENTIONS (PROJECT-SPECIFIC)

- Rust commands run from the repo root (the cargo workspace root); stop if `Cargo.toml` absent.
- Portable engine, protocol, persistence, migration, and binding changes belong in `UniClipboard/Engine`; never recreate those packages here.
- 升级引擎时，只修改根目录 `Cargo.toml` 中唯一的固定发布标签，并同步更新 `Cargo.lock`。
- Desktop-only capability flow: platform adapter -> `uc-bootstrap/src/wiring/` -> `HostCapabilities` -> `Engine::start`.
- Host handlers (Go) and daemon HTTP routes call app-layer use cases; avoid direct `deps` access from the transport layer.
- Event payloads sent to the frontend must use camelCase field names.
- Use `tracing` structured logs; avoid `println!/eprintln!/log` macros in production.
- 做产品/架构方向判断前先读根目录 `VISION.md`。

- Daemon binds an ephemeral loopback port and publishes host/port/token/pid in `<app_data_root>/daemon.conn` (`0o600`, atomic write; ADR-011). Clients discover the daemon through this file only; the legacy `UC_PROFILE`-hash fixed port is retired.
- Daemon auth flow: Bearer file-token → `POST /auth/connect` `{"pid":N,"clientType":"cli"}` → Session JWT; use `Session <jwt>` header afterward.
- `POST /clipboard/dispatch` sends to peers only; dispatched content does NOT appear in sender's `/clipboard/entries` (entries come from OS clipboard captures).

## ANTI-PATTERNS (THIS PROJECT)

- Copying core source, migrations, bindings, or LAN protocol packages back into desktop.
- 对 `UniClipboard/Engine` 拥有的包使用本地路径、分支或非发布标签。
- Depending on core implementation packages from desktop production code instead of `uc-engine`.
- Adding business logic inside host command handlers or platform adapters.
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
cargo build -p uc-daemon -p uc-dev-cli
cargo test --manifest-path tests/e2e/Cargo.toml -- --ignored

# Coverage wrapper (from repo root)
bun run test:coverage
```

## NOTES

- `src-legacy/` was removed on 2026-02-26; treat any references as historical context only.
- Root `AGENTS.md` is the navigation index; this file is the Rust-workspace knowledge base covering `crates/` and `apps/`. Desktop host details live in `apps/gui-go/AGENTS.md`.
- Any change touching `crates/uc-platform/src/clipboard/` (especially the Linux X11/Wayland adapters) should run the package's focused validation before merge.
- Engine and LAN compatibility releases are produced only by `UniClipboard/Engine`; desktop keeps no mobile binding source or release workflow.
- Log files live in the platform-conventional log location (separate from the data root since the logs split). Single source of truth: `uc_app_paths::app_log_dir()`. Per-role files `uniclipboard-{gui,daemon,cli}.json.<date>`, daily rotation, 7-day retention (older pruned on start).
- macOS: `~/Library/Logs/app.uniclipboard.desktop[-<profile>]/`
- Linux: `~/.local/state/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows: `%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- Portable ("green") builds keep logs under `<exe>/data/logs/`.
- Older legacy app-data roots may still exist from previous builds, but they are not the current default.
