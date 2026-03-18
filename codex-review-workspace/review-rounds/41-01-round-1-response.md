# Codex Review: Phase 41-01 — Round 1

**Model:** gpt-5.4
**Date:** 2026-03-18
**Session ID:** 019d010f-bcd1-7822-ae58-ddf6c2884dd9

---

## Findings

FINDING-1: CRITICAL
The plan allows Task 1 to optionally change `CoreRuntime`'s `setup_orchestrator` field to `Option<Arc<SetupOrchestrator>>`. This would escalate a non-GUI assembly concern in `uc-bootstrap` into a public runtime contract change in `uc-app`, expanding Phase 41-01's scope and introducing branching/regression surface for all GUI/non-GUI paths. The repository already has reusable `build_setup_orchestrator()` and `SetupAssemblyPorts::placeholder(deps)` — no need to modify the `CoreRuntime` signature.
SUGGESTION: Explicitly forbid modifying `CoreRuntime` public fields and constructor signature in this plan. Task 1 should use `uc-bootstrap::build_setup_orchestrator()` + `SetupAssemblyPorts::placeholder(deps)` to assemble the non-GUI `SetupOrchestrator`, keeping complexity in `uc-bootstrap` without leaking into `uc-app`.

FINDING-2: MAJOR
The plan's `build_non_gui_runtime(deps, storage_paths)` cannot be stably consumed by the CLI path because the current `CliBootstrapContext` only has `deps` and `config` — it does not include `storage_paths`. This means 41-01 claims to provide "shared CLI/daemon prerequisites" but leaves a missing required input for CLI, forcing Plan 03 to modify the bootstrap API again.
SUGGESTION: In 41-01, commit to one of two approaches: either add `storage_paths: AppPaths` to `CliBootstrapContext`, or change `build_non_gui_runtime()` to accept `&AppConfig`/`CliBootstrapContext` and call `get_storage_paths()` internally. Do not leave this gap for later plans.

FINDING-3: MAJOR
`RuntimeState` is designed to hold `Vec<Box<dyn DaemonWorker>>`, combining "state snapshot" and "worker ownership" responsibilities. This will cause conflicts in Plan 02 where RPC status queries need concurrent reads while the daemon needs to start/stop/health-check workers. Putting trait objects in `RuntimeState` forces coarse-grained locking or makes it difficult to safely dispatch long-lived `start(&self)` futures.
SUGGESTION: Keep worker ownership in `DaemonApp`. `RuntimeState` should only hold concurrency-safe snapshot data (e.g., `start_time`, `Arc<RwLock<Vec<WorkerStatus>>>`, `connected_peers`). If shared worker references are needed, at minimum use `Vec<Arc<dyn DaemonWorker>>` and define a concurrency access strategy in the plan.

FINDING-4: MAJOR
Verification is insufficient. The current 41-01 plan only requires `cargo check`, with no coverage for whether `build_non_gui_runtime()` actually constructs a `CoreRuntime`, whether `LoggingHostEventEmitter` returns `Ok(())` for all `HostEvent` variants, whether `RuntimeState::worker_statuses()` generates snapshots as expected, or whether RPC types round-trip through serde correctly. For a foundation layer that two subsequent crates depend on, compile-only checks will defer design errors to Plan 02/03.
SUGGESTION: Add at minimum one unit test per module: test `build_non_gui_runtime()` successfully constructs in `uc-bootstrap`; test `LoggingHostEventEmitter` returns `Ok(())` for at least one `HostEvent` variant; test `RuntimeState::uptime_seconds/worker_statuses` in `uc-daemon`; add a JSON serialize/deserialize round-trip assertion for `rpc/types.rs`.

FINDING-5: MINOR
The placeholder `src/main.rs` uses `eprintln!` for "not yet implemented" output. This violates the repository's convention of using `tracing` uniformly for production code, and introduces non-standard output behavior in what should be a structural skeleton.
SUGGESTION: Either omit the placeholder binary entirely (defer to Plan 02), or design the placeholder `main` as `fn main() -> anyhow::Result<()>` that initializes tracing first, logs the unimplemented state via structured logging, and returns an explicit error code.

---

VERDICT: NEEDS_REVISION — the issues above must be addressed

---

_Tokens used: 51,591_
