# PROJECT KNOWLEDGE BASE

**Generated:** 2026-02-10 21:24 (Asia/Shanghai)
**Commit:** `86a75825`
**Branch:** `feat/join-space`

## OVERVIEW

Tauri v2 desktop backend with strict hexagonal boundaries. Runtime entry is thin (`src/main.rs`), domain/app/infra/platform live in workspace crates.

## STRUCTURE

```text
src-tauri/
|- src/                  # Tauri shell entry, plugin wiring, invoke_handler registration
|- crates/               # Hex architecture workspace
|  |- uc-core/           # Domain model + Port traits only
|  |- uc-app/            # Use cases / orchestrators
|  |- uc-infra/          # DB/FS/security adapters
|  |- uc-platform/       # OS/network/platform adapters
|  `- uc-tauri/          # Tauri adapters, commands, bootstrap wiring
|- src-legacy/           # Migration reference only (do not add new logic)
|- migrations/           # Legacy diesel migrations
`- crates/uc-infra/migrations/ # Active infra migrations
```

## WHERE TO LOOK

| Task                             | Location                                   | Notes                                                         |
| -------------------------------- | ------------------------------------------ | ------------------------------------------------------------- |
| App startup & state registration | `src/main.rs`                              | `run_app`, `.manage(...)`, `.setup(...)`, `invoke_handler![]` |
| Dependency composition           | `crates/uc-tauri/src/bootstrap/wiring.rs`  | Port-to-adapter injection center                              |
| Runtime/usecase accessors        | `crates/uc-tauri/src/bootstrap/runtime.rs` | `AppRuntime`, `usecases()` factory                            |
| Tauri commands                   | `crates/uc-tauri/src/commands/`            | Commands call app-layer usecases                              |
| Domain contracts (ports)         | `crates/uc-core/src/ports/`                | Add traits here first                                         |
| App workflows                    | `crates/uc-app/src/usecases/`              | Pairing/setup/space_access orchestration                      |
| Infra implementations            | `crates/uc-infra/src/`                     | Diesel repos, encryption, fs, timers                          |
| Platform adapters                | `crates/uc-platform/src/`                  | libp2p, clipboard, secure storage                             |
| Legacy reference                 | `src-legacy/`                              | Reference-only; no new code                                   |

## CODE MAP

| Symbol                     | Type       | Location                                   | Role                               |
| -------------------------- | ---------- | ------------------------------------------ | ---------------------------------- |
| `main`                     | fn         | `src/main.rs`                              | Process entrypoint                 |
| `run_app`                  | fn         | `src/main.rs`                              | Tauri builder + state registration |
| `wire_dependencies`        | fn         | `crates/uc-tauri/src/bootstrap/wiring.rs`  | Hex boundary composition           |
| `AppRuntime::with_setup`   | fn         | `crates/uc-tauri/src/bootstrap/runtime.rs` | Runtime and usecase host           |
| `tauri::generate_handler!` | macro site | `src/main.rs`                              | Command registration list          |

## CONVENTIONS (PROJECT-SPECIFIC)

- Rust commands run from `src-tauri/` only; stop if `Cargo.toml` absent.
- Keep `uc-core` pure; no infra/platform dependencies in core.
- New external capability flow: `uc-core/ports` trait -> adapter in `uc-infra` or `uc-platform` -> wire in `uc-tauri/bootstrap/wiring.rs`.
- Tauri command pattern: command -> `runtime.usecases().x()`; avoid direct `deps` access from command layer.
- Event payloads emitted via `app.emit()` must use `#[serde(rename_all = "camelCase")]`.
- Use `tracing` structured logs; avoid `println!/eprintln!/log` macros in production.
- For libp2p/event-loop changes, preserve non-blocking poll loop progress; do not block swarm progression while awaiting business stream operations.

## ANTI-PATTERNS (THIS PROJECT)

- Mixing boundary layers in one change set (`uc-core` + `uc-infra` etc.).
- Adding business logic inside `uc-tauri` command handlers or platform adapters.
- New code under `src-legacy/`.
- Introducing `unwrap()/expect()` in production paths.
- Emitting snake_case payload fields to frontend events.

## COMPLEXITY HOTSPOTS

- `crates/uc-tauri/src/bootstrap/wiring.rs`: global wiring and emit loops; smallest safe edits only.
- `crates/uc-app/src/usecases/setup/orchestrator.rs`: high-state async setup transitions.
- `crates/uc-core/src/network/pairing_state_machine.rs`: protocol-critical state machine.
- `crates/uc-app/src/usecases/pairing/orchestrator.rs`: side-effect orchestration around pairing FSM.
- `crates/uc-platform/src/adapters/libp2p_network.rs`: transport internals; keep business rules out.

## COMMANDS

```bash
# Workspace checks (from src-tauri/)
cargo check --workspace
cargo test --workspace

# Targeted package quick loop
make check
make build

# Coverage wrapper (from repo root)
bun run test:coverage
```

## NOTES

- `src-legacy/` is not in active runtime path; treat as migration archive.
- Current repository root also has parent-level `AGENTS.md`; local file narrows rules to `src-tauri/` workspace details.
- Any change touching `crates/uc-platform/src/adapters/libp2p_network.rs` must run `cargo test -p uc-platform` before merge.
